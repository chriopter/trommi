//! The Scribble Board's merge (section 10): items of up to four writers, each writer's in the order of its
//! chain, arriving interleaved in any order and some twice, give one board. A snapshot taken anywhere on the
//! way and read back, with the rest of the items applied to it, gives the same board again.

use super::Bytes;
use crate::content::{shape, Dice};
use trommi_core::board::ShapeId;
use trommi_core::board_items::{Applied, Board, ItemBody};
use trommi_core::chain::Head;
use trommi_core::ids::{DeviceId, Hash32};

struct Item {
    sender: DeviceId,
    seq: u64,
    payload: Vec<u8>,
}

fn apply(board: &mut Board, item: &Item) -> Applied {
    let body = ItemBody::decode(&item.payload).expect("a written body reads");
    board
        .apply(item.sender, item.seq, body)
        .expect("a checked body applies")
}

pub fn run(data: &[u8]) {
    let mut dice = Dice(Bytes::new(data));
    let n_writers = 1 + dice.below(4).unwrap_or(0) as usize;
    let writers: Vec<DeviceId> = (0..n_writers)
        .map(|i| DeviceId::new([0x10 + i as u8; 32]))
        .collect();
    let mut last = vec![0u64; n_writers];
    let mut items = Vec::new();
    let len = dice.below(40).unwrap_or(0);
    for _ in 0..len {
        let Ok(who) = dice.below(n_writers as u64) else {
            break;
        };
        let who = who as usize;
        last[who] += 1 + dice.below(3).unwrap_or(0);
        let mut targets: Vec<ShapeId> = Vec::new();
        for _ in 0..1 + dice.below(4).unwrap_or(0) {
            let of = dice.below(n_writers as u64).unwrap_or(0) as usize;
            let id = ShapeId {
                sender: writers[of],
                seq: 1 + dice.below(last[of] + 3).unwrap_or(0),
                index: dice.below(3).unwrap_or(0) as u32,
            };
            if !targets.contains(&id) {
                targets.push(id);
            }
        }
        let body = match dice.below(10).unwrap_or(0) {
            0..=4 => {
                let mut shapes = Vec::new();
                for _ in 0..1 + dice.below(3).unwrap_or(0) {
                    match shape(&mut dice) {
                        Ok(s) => shapes.push(s),
                        Err(_) => break,
                    }
                }
                if shapes.is_empty() {
                    continue;
                }
                ItemBody::Strokes(shapes)
            }
            5 | 6 => ItemBody::Erase(targets),
            7 => ItemBody::SendAway(targets),
            _ => ItemBody::Move {
                shapes: targets,
                offset: match dice.below(4).unwrap_or(0) {
                    0 => [i32::MAX, i32::MIN],
                    1 => [
                        dice.between(i32::MIN, i32::MAX).unwrap_or(0),
                        dice.between(i32::MIN, i32::MAX).unwrap_or(0),
                    ],
                    _ => [
                        dice.between(-400, 400).unwrap_or(0),
                        dice.between(-400, 400).unwrap_or(0),
                    ],
                },
            },
        };
        let payload = body
            .encode()
            .expect("a made body encodes")
            .expose()
            .to_vec();
        assert_eq!(
            ItemBody::decode(&payload).expect("its encoding decodes"),
            body,
            "item body: decode(encode(x)) != x"
        );
        items.push(Item {
            sender: writers[who],
            seq: last[who],
            payload,
        });
    }

    // in the order written
    let mut first = Board::new();
    for item in &items {
        apply(&mut first, item);
    }

    // in another order: each writer's own in order, between writers at random, some again
    let mut queues: Vec<std::collections::VecDeque<&Item>> = writers
        .iter()
        .map(|w| items.iter().filter(|i| i.sender == *w).collect())
        .collect();
    let mut order: Vec<&Item> = Vec::new();
    while order.len() < items.len() {
        let from = dice.below(n_writers as u64).unwrap_or(0) as usize;
        let from = if queues[from].is_empty() {
            queues
                .iter()
                .position(|q| !q.is_empty())
                .expect("items left")
        } else {
            from
        };
        order.extend(queues[from].pop_front());
    }
    let cut = dice.below(order.len() as u64 + 1).unwrap_or(0) as usize;
    let mut second = Board::new();
    let mut seen: Vec<&Item> = Vec::new();
    for (place, item) in order.iter().enumerate() {
        if place == cut {
            second = through_snapshot(&second, &writers);
        }
        assert_ne!(
            apply(&mut second, item),
            Applied::Covered,
            "a new item was covered"
        );
        seen.push(item);
        if dice.one_in(4).unwrap_or(false) {
            // a replay of something this writer sent before changes nothing
            let earlier: Vec<&&Item> = seen.iter().filter(|i| i.sender == item.sender).collect();
            let pick = dice.below(earlier.len() as u64).unwrap_or(0) as usize;
            assert_eq!(
                apply(&mut second, earlier[pick]),
                Applied::Covered,
                "a replay was applied"
            );
        }
    }
    if cut == order.len() {
        second = through_snapshot(&second, &writers);
    }
    assert_eq!(
        first, second,
        "two orders of the same items gave two boards"
    );
}

/// The board as a device sees it that loads its snapshot.
fn through_snapshot(board: &Board, writers: &[DeviceId]) -> Board {
    let frontier: Vec<(DeviceId, Head)> = writers
        .iter()
        .filter(|w| board.applied(w) > 0)
        .map(|w| {
            (
                *w,
                Head {
                    seq: board.applied(w),
                    hash: Hash32::new([0xAB; 32]),
                },
            )
        })
        .collect();
    let file = board
        .snapshot(&frontier)
        .expect("a board writes its snapshot");
    let back = Board::from_snapshot(file.expose(), &frontier).expect("its snapshot reads");
    assert_eq!(&back, board, "board snapshot: decode(encode(x)) != x");
    back
}
