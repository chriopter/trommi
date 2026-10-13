//! The live stream's parser: framing that does not depend on where the bytes are cut, events checked as strictly
//! as answers, bounds on a line and an event.

use trommi_client_core::app::Header;
use trommi_client_core::hub::limits::CAP_EVENT;
use trommi_client_core::hub::{
    ChangeItem, Codec, Failure, Route, Site, StreamEvent, StreamReader, WishRowKind,
};
use trommi_core::ids::base64url_encode as b64;

fn envelope(change: u64) -> String {
    format!(
        "id: {change}\nevent: envelope\ndata: {{\"change\":{change},\"kind\":\"envelope\",\"envelope\":\"AAEC\",\"received_at\":5}}\n\n"
    )
}

fn commit(change: u64) -> String {
    let (group, sender) = (b64(&[7; 48]), b64(&[9; 32]));
    format!(
        "id: {change}\nevent: log\ndata: {{\"change\":{change},\"kind\":\"commit\",\"group_id\":\"{group}\",\"n\":1,\"epoch\":0,\"at\":5,\"bytes\":\"AAEC\",\"sender\":\"{sender}\"}}\n\n"
    )
}

/// A stream with every kind of event, comments and a ping between them.
fn stream() -> String {
    let (device, group, file, invite) =
        (b64(&[3; 32]), b64(&[7; 48]), b64(&[4; 16]), b64(&[5; 16]));
    [
        format!("event: presence\ndata: {{\"device\":\"{device}\",\"online\":true,\"hears\":true,\"working\":false,\"last_call_at\":77}}\n\n"),
        "event: here\ndata: {}\n\n".to_owned(),
        ": a comment\n\n".to_owned(),
        envelope(11),
        "event: ping\ndata: {}\n\n".to_owned(),
        commit(12),
        format!("event: relay\ndata: {{\"group_id\":\"{group}\",\"epoch\":2,\"sender\":\"{device}\",\"message\":\"AAEC\"}}\n\n"),
        format!("event: welcome\ndata: {{\"group_id\":\"{group}\"}}\n\n"),
        format!("event: request\ndata: {{\"kind\":\"readmit\",\"id\":4,\"device\":\"{device}\",\"group_id\":\"{group}\"}}\n\n"),
        format!("event: request\ndata: {{\"kind\":\"invite\",\"invite_id\":\"{invite}\"}}\n\n"),
        format!("event: presence\ndata: {{\"group_id\":\"{group}\",\"archived\":true}}\n\n"),
        format!("event: presence\ndata: {{\"device\":\"{device}\",\"online\":false,\"lost\":true}}\n\n"),
        format!("event: file_evicted\ndata: {{\"file_id\":\"{file}\"}}\n\n"),
        "event: of_a_later_hub\ndata: [whatever\n\n".to_owned(),
        envelope(15),
    ]
    .concat()
}

/// Feeds `bytes` cut at `cuts` and returns what came out, as text (events have no `Eq` on their bytes' meaning
/// beyond `PartialEq`, and the text shows a difference at once).
fn feed(
    reader: &mut StreamReader,
    bytes: &[u8],
    cuts: &[usize],
) -> (Vec<StreamEvent>, Option<Failure>) {
    let mut events = Vec::new();
    let mut from = 0;
    for cut in cuts.iter().copied().chain([bytes.len()]) {
        let fed = reader.chunk(&bytes[from..cut]);
        events.extend(fed.events);
        if fed.failure.is_some() {
            return (events, fed.failure);
        }
        from = cut;
    }
    (events, None)
}

#[test]
fn every_kind_of_event_is_read() {
    let mut reader = StreamReader::new(10);
    let (events, failure) = feed(&mut reader, stream().as_bytes(), &[]);
    assert_eq!(failure, None);
    assert_eq!(events.len(), 12, "{events:?}");
    assert!(
        matches!(&events[0], StreamEvent::Presence(p) if p.online && p.hears == Some(true) && p.working == Some(false) && p.last_call_at == Some(77) && !p.lost)
    );
    assert!(matches!(&events[1], StreamEvent::Here));
    assert!(
        matches!(&events[2], StreamEvent::Change(ChangeItem::Envelope(e)) if e.change == 11 && e.envelope == [0, 1, 2])
    );
    assert!(matches!(&events[3], StreamEvent::Change(ChangeItem::Commit(l)) if l.change == 12));
    assert!(
        matches!(&events[4], StreamEvent::Relay { epoch: 2, message, .. } if message == &[0, 1, 2])
    );
    assert!(matches!(&events[5], StreamEvent::Welcome { .. }));
    assert!(
        matches!(&events[6], StreamEvent::Wish(w) if w.id == 4 && w.kind == WishRowKind::Readmit && w.group.is_some() && w.n.is_none())
    );
    assert!(matches!(&events[7], StreamEvent::InviteRequest { .. }));
    assert!(matches!(&events[8], StreamEvent::Archived { .. }));
    assert!(
        matches!(&events[9], StreamEvent::Presence(p) if !p.online && p.lost && p.hears.is_none())
    );
    assert!(matches!(&events[10], StreamEvent::FileEvicted { .. }));
    assert!(matches!(&events[11], StreamEvent::Change(ChangeItem::Envelope(e)) if e.change == 15));
    assert_eq!(reader.last_change(), 15);
    assert_eq!(reader.ended(), None);
}

#[test]
fn chunks_cut_at_every_byte_position_give_the_same_events() {
    // the three line ends of server-sent events, and a bad event at the end: the failure is part of the result
    let good = stream();
    let bad_end = format!("{good}{}", envelope(15));
    for text in [
        good.clone(),
        good.replace('\n', "\r\n"),
        good.replace('\n', "\r"),
        bad_end,
    ] {
        let bytes = text.as_bytes();
        let whole = feed(&mut StreamReader::new(10), bytes, &[]);
        assert!(whole.0.len() >= 12);
        for cut in 0..=bytes.len() {
            let got = feed(&mut StreamReader::new(10), bytes, &[cut]);
            assert_eq!(got, whole, "cut at {cut}");
        }
        // byte by byte, and in threes
        for step in [1, 3] {
            let cuts: Vec<usize> = (step..bytes.len()).step_by(step).collect();
            assert_eq!(
                feed(&mut StreamReader::new(10), bytes, &cuts),
                whole,
                "every {step}"
            );
        }
    }
}

#[test]
fn the_framing_of_server_sent_events() {
    let read = |text: &str| feed(&mut StreamReader::new(0), text.as_bytes(), &[]);
    // no space after the colon; data over two lines is joined by a line feed (inside JSON: white space)
    let (events, failure) = read("event:envelope\nid:3\ndata:{\"change\":3,\"kind\":\"envelope\",\ndata:\"envelope\":\"AAEC\",\"received_at\":5}\n\n");
    assert_eq!((events.len(), failure), (1, None));
    // an event without a name, an unknown field, a field without a colon, a data-only event: passed over
    let (events, failure) =
        read("data: {}\n\nretry: 5\n\nevent\n\nevent: here\nfoo: bar\ndata: {}\n\n");
    assert_eq!((events.len(), failure), (1, None));
    // an event that the end of the stream cuts short is not an event
    let mut reader = StreamReader::new(0);
    let (events, failure) = feed(&mut reader, b"event: here\ndata: {}\n", &[]);
    assert_eq!((events.len(), failure, reader.ended()), (0, None, None));
    // after the end nothing is taken
    assert!(reader.chunk(b"event: here\ndata: {}\n\n").failure.is_some());
}

#[test]
fn an_event_the_protocol_does_not_allow_ends_the_stream() {
    let device = b64(&[3; 32]);
    let bad = |text: String| {
        let mut reader = StreamReader::new(10);
        let (events, failure) = feed(&mut reader, text.as_bytes(), &[]);
        assert!(
            matches!(failure, Some(Failure::BadAnswer { .. })),
            "taken: {text}"
        );
        // and nothing after it
        let after = reader.chunk(envelope(99).as_bytes());
        assert!(after.events.is_empty() && after.failure.is_some());
        events.len()
    };
    // a change sent before, at the cursor, out of order; the good one before it is still handed over
    assert_eq!(bad(envelope(10)), 0);
    assert_eq!(bad(envelope(9)), 0);
    assert_eq!(bad(format!("{}{}", envelope(12), envelope(12))), 1);
    assert_eq!(bad(format!("{}{}", envelope(12), envelope(11))), 1);
    // an id that is not the change number, no id, a name that is not the kind
    bad(envelope(11).replace("id: 11", "id: 12"));
    bad(envelope(11).replace("id: 11", "id: 011"));
    bad(envelope(11).replace("id: 11\n", ""));
    bad(envelope(11).replace("event: envelope", "event: log"));
    bad(commit(11).replace("event: log", "event: envelope"));
    // data that is not the JSON of its kind
    bad("event: here\ndata: not json\n\n".to_owned());
    bad("event: here\ndata: []\n\n".to_owned());
    bad("event: here\n\n".to_owned());
    bad("event: welcome\ndata: {\"group_id\":\"AAEC\"}\n\n".to_owned());
    bad(format!(
        "event: presence\ndata: {{\"device\":\"{device}\",\"online\":1}}\n\n"
    ));
    bad(format!(
        "event: presence\ndata: {{\"device\":\"{device}\",\"online\":true,\"hears\":\"yes\"}}\n\n"
    ));
    bad(format!(
        "event: request\ndata: {{\"kind\":\"other\",\"id\":4,\"device\":\"{device}\"}}\n\n"
    ));
    bad("event: file_evicted\ndata: {\"file_id\":7}\n\n".to_owned());
    bad("event: here\ndata: {\"a\":1,\"a\":2}\n\n".to_owned());
    let mut reader = StreamReader::new(0);
    assert!(reader
        .chunk(b"event: \xff\xfe\ndata: {}\n\n")
        .failure
        .is_some());
}

#[test]
fn a_line_and_an_event_are_bounded() {
    // one line that never ends
    let mut reader = StreamReader::new(0);
    let block = vec![b'x'; 1 << 20];
    let mut fed = 0;
    let failure = loop {
        let got = reader.chunk(&block);
        fed += block.len();
        if let Some(failure) = got.failure {
            break failure;
        }
        assert!(fed <= CAP_EVENT, "took more than an event may hold");
    };
    assert!(matches!(failure, Failure::BadAnswer { .. }));
    // an event of many small lines
    let mut reader = StreamReader::new(0);
    let line = b"data: 0123456789\n".repeat(1 << 12);
    let mut fed = 0;
    while reader.chunk(&line).failure.is_none() {
        fed += line.len();
        assert!(
            fed <= CAP_EVENT + line.len(),
            "took more than an event may hold"
        );
    }
    // comments between events are no event: a keep-alive that never sends an empty line does not add up
    let mut reader = StreamReader::new(0);
    let comments = b": keep-alive\n".repeat(1 << 16);
    for _ in 0..8 {
        assert_eq!(reader.chunk(&comments).failure, None);
    }
    assert_eq!(reader.chunk(envelope(1).as_bytes()).events.len(), 1);
}

/// A `here` event of exactly `size` bytes, from its first line to its empty one, lines ended by `end`: the room
/// is filled with lines of a field that is none of the protocol's, `line` bytes each (the last one shorter).
fn event_of(size: usize, end: &str, line: usize) -> Vec<u8> {
    let mut out = format!("event: here{end}data: {{}}{end}").into_bytes();
    let last = size - end.len();
    while out.len() < last {
        let room = last - out.len();
        // (never a rest too short for a line of one byte and its end)
        let take = match room.saturating_sub(line + end.len()) {
            rest if rest > end.len() || rest == 0 => line.min(room - end.len()),
            _ => room - end.len(),
        };
        out.extend(std::iter::repeat_n(b'x', take));
        out.extend_from_slice(end.as_bytes());
    }
    out.extend_from_slice(end.as_bytes());
    assert_eq!(out.len(), size);
    out
}

#[test]
fn every_byte_of_an_event_counts() {
    // an event may hold CAP_EVENT bytes and not one more, whatever its lines end in and however short they are
    // (a line of one byte is half or two thirds line end: those bytes count as the others do)
    for end in ["\n", "\r\n", "\r"] {
        for line in [1, 61, CAP_EVENT] {
            let whole = event_of(CAP_EVENT, end, line);
            for cut in [whole.len(), 1 << 16, 4099] {
                let mut reader = StreamReader::new(0);
                let mut events = Vec::new();
                for chunk in whole.chunks(cut) {
                    let got = reader.chunk(chunk);
                    assert_eq!(got.failure, None, "{end:?} {line} {cut}");
                    events.extend(got.events);
                }
                assert_eq!(events, [StreamEvent::Here], "{end:?} {line} {cut}");
                // the stream goes on: the next event has its own room
                let next = reader.chunk(&whole);
                assert_eq!((next.events.len(), next.failure), (1, None));
            }
            // (the LF of a CRLF that ends an event is read when the event is over: the one byte not counted)
            let more = event_of(CAP_EVENT + end.len(), end, line);
            for cut in [more.len(), 1 << 16, 4099] {
                let mut reader = StreamReader::new(0);
                let mut failed = false;
                for chunk in more.chunks(cut) {
                    let got = reader.chunk(chunk);
                    assert!(got.events.is_empty(), "{end:?} {line} {cut}");
                    failed |= got.failure.is_some();
                }
                assert!(failed, "{end:?} {line} {cut}");
            }
        }
    }
    // one chunk far beyond an event, with and without a line's end in it
    for tail in ["", "\n\n"] {
        let mut chunk = vec![b'x'; 3 * CAP_EVENT];
        chunk.extend_from_slice(tail.as_bytes());
        let got = StreamReader::new(0).chunk(&chunk);
        assert!(got.events.is_empty() && got.failure.is_some());
    }
}

#[test]
fn what_a_catch_up_took_is_not_handed_over_twice() {
    let mut reader = StreamReader::new(10);
    reader.caught_up_to(12);
    let text = [envelope(11), envelope(12), envelope(13)].concat();
    let (events, failure) = feed(&mut reader, text.as_bytes(), &[]);
    assert_eq!(failure, None);
    assert!(matches!(&events[..], [StreamEvent::Change(item)] if item.change() == 13));
    assert_eq!(reader.last_change(), 13);
    // still checked for their order
    let mut reader = StreamReader::new(10);
    reader.caught_up_to(20);
    let text = [envelope(12), envelope(11)].concat();
    assert!(feed(&mut reader, text.as_bytes(), &[]).1.is_some());
}

#[test]
fn the_request_and_the_answers_head() {
    let site = Site {
        url: "https://hub.example",
        token: Some("abcdefghijklmnop_-0123"),
        client: "web/1.2.3",
    };
    let (r, expect) = Codec.request(&site, &Route::Stream { after: 44 }).unwrap();
    assert_eq!(
        (r.method.as_str(), r.url.as_str()),
        ("GET", "https://hub.example/v1/stream?after=44")
    );
    assert!(r
        .headers
        .iter()
        .any(|h| h.name == "accept" && h.value == "text/event-stream"));
    assert!(r.headers.iter().any(|h| h.name == "authorization"));
    assert_eq!(r.max_answer, 64 << 10);
    // a stream has no answer to read
    let response = trommi_client_core::app::HttpResponse {
        status: 200,
        headers: vec![],
        body: Vec::new().into(),
    };
    assert!(Codec.answer(&expect, &response).is_err());

    let head = |name: &str, value: &str| {
        vec![Header {
            name: name.to_owned(),
            value: value.to_owned(),
        }]
    };
    assert_eq!(
        StreamReader::new(0).opened(
            200,
            &head("Content-Type", "text/event-stream; charset=utf-8")
        ),
        Ok(())
    );
    let mut reader = StreamReader::new(0);
    assert!(reader
        .opened(200, &head("content-type", "text/html"))
        .is_err());
    assert!(reader.chunk(b"event: here\ndata: {}\n\n").failure.is_some());
    assert!(StreamReader::new(0).opened(200, &[]).is_err());

    // an answer that is no 200: its body is collected and sorted when it ends, as any answer's
    let mut reader = StreamReader::new(0);
    assert_eq!(
        reader.opened(403, &head("content-type", "application/json")),
        Ok(())
    );
    let body = br#"{"error":"not-member","message":"no"}"#;
    assert_eq!(feed(&mut reader, body, &[5, 20]), (vec![], None));
    assert_eq!(
        reader.ended(),
        Some(Failure::Refused {
            status: 403,
            code: Some("not-member".into())
        })
    );
    let mut reader = StreamReader::new(0);
    reader.opened(429, &head("Retry-After", "7")).unwrap();
    assert_eq!(
        reader.ended(),
        Some(Failure::NotNow {
            status: 429,
            retry_after_ms: Some(7000)
        })
    );
    // a body that never ends is not kept
    let mut reader = StreamReader::new(0);
    reader.opened(401, &[]).unwrap();
    for _ in 0..64 {
        assert_eq!(reader.chunk(&[b' '; 1 << 16]).failure, None);
    }
    assert_eq!(
        reader.ended(),
        Some(Failure::Refused {
            status: 401,
            code: None
        })
    );
    // 204 is no stream either
    let mut reader = StreamReader::new(0);
    reader.opened(204, &[]).unwrap();
    assert!(matches!(reader.ended(), Some(Failure::BadAnswer { .. })));
}
