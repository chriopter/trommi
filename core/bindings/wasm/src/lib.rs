//! trommi-core for the browser: every call of the shared facade (`core/bindings/swift/src`) as a wasm-bindgen export.
//!
//! This layer is mechanical. Each export takes JavaScript values, turns them into the facade's plain data
//! (`trommi_core_ffi::js`: a `Uint8Array` for bytes, a plain object for a record, a `number` for a count), calls
//! the facade's function of the same name, and turns the answer back: a pair, `[null, value]` or, for a refusal,
//! `[code, message]` with the stable code of the specification's section 16.
//!
//! The exports keep the facade's names. The hand-written module `js/trommi-core.js` is what an app imports: it
//! gives them their JavaScript spelling, stores what a device's call wrote before the call's result reaches
//! anyone, and closes everything when the module stops.
//!
//! A panic stops the module (WebAssembly has no unwinding here): the export throws a `RuntimeError`, and the
//! JavaScript layer refuses every later call.
//!
//! Built for another target than the browser's, this crate is empty.

#![cfg(target_arch = "wasm32")]

use js_sys::Array;
use trommi_core_ffi as facade;
use trommi_core_ffi::js::{FromJs, ToJs};
use trommi_core_ffi::{
    ClientDesk, ClientModel, CoreDevice, CoreError, FileDecryptor, FileEncryptor,
};
use wasm_bindgen::prelude::*;

/// What every export returns: a pair. `[null, value]` when the call went through, `[code, message]` when the
/// core refused it. Nothing is thrown on purpose, so whatever an export does throw is a fault (a panic, a misuse
/// of the glue), and the JavaScript layer treats it as one: a refusal cannot be forged by an exception.
type Answer = Array;

fn pair(first: &JsValue, second: &JsValue) -> Answer {
    Array::of2(first, second)
}

fn refusal(error: &CoreError) -> Answer {
    pair(
        &JsValue::from_str(error.code().text()),
        &JsValue::from_str(error.message()),
    )
}

/// An argument as the facade takes it.
fn arg<T: FromJs>(value: &JsValue) -> Result<T, CoreError> {
    T::from_js(value)
}

/// Runs a call whose arguments may themselves be refused.
fn attempt<T>(call: impl FnOnce() -> Result<T, CoreError>) -> Result<T, CoreError> {
    call()
}

/// An answer as JavaScript takes it.
fn answer<T: ToJs>(result: Result<T, CoreError>) -> Answer {
    match result.and_then(|value| value.to_js()) {
        Ok(value) => pair(&JsValue::NULL, &value),
        Err(error) => refusal(&error),
    }
}

/// The facade's free functions, by name and arguments. `fallible` ones return a `Result`, `plain` ones cannot
/// fail.
macro_rules! functions {
    ($($kind:ident $name:ident($($argument:ident),*);)*) => {
        $(functions!(@one $kind $name($($argument),*));)*
    };
    (@one fallible $name:ident($($argument:ident),*)) => {
        #[wasm_bindgen]
        pub fn $name($($argument: &JsValue),*) -> Answer {
            answer(attempt(|| facade::$name($(arg($argument)?),*)))
        }
    };
    (@one plain $name:ident($($argument:ident),*)) => {
        #[wasm_bindgen]
        pub fn $name($($argument: &JsValue),*) -> Answer {
            answer(attempt(|| Ok(facade::$name($(arg($argument)?),*))))
        }
    };
}

functions! {
    plain versions();
    plain self_test(now_ms);
    plain log_finding(code);
    plain error_code_text(code);
    plain error_code_from_text(text);
    fallible key_package_info(key_package);
    fallible room_group_id(room);
    fallible session_group_id(room, session);
    plain base64url_encode(bytes);
    fallible base64url_decode(text);
    fallible file_layout(stored_len);
    fallible file_chunk(stored_len, index);
    fallible open_file_chunk(file_key, head, index, last, sealed);
    fallible share_link_create(app, file);
    fallible share_link_parse(text);
    fallible check_share_expiry(expires_at, now_ms);
    fallible normalise_email(email);
    fallible check_password(password);
    plain kdf_record();
    fallible password_keys(email, password, kdf);
    fallible kit_keys(email, words);
    fallible passkey_wrap_key(prf, room_id, credential_id);
    plain passkey_prf_input();
    fallible seal_recovery_code(wrap_key, room_id, way_in, credential_id, recovery_code);
    fallible open_recovery_code(wrap_key, room_id, way_in, credential_id, sealed);
    fallible generate_recovery_code();
    fallible format_recovery_code(recovery_code);
    fallible parse_recovery_code(text);
    fallible generate_kit_words();
    fallible parse_kit_words(text);
    fallible generate_user_handle();
    fallible generate_push_key();
    fallible open_apns_push(key, sealed);
    fallible open_live_activity(key, sealed);
    fallible read_web_push(payload);
    fallible open_notify_envelope(keys, envelope);
    fallible recovery_anchor(recovery_code, room, rows);
    fallible recovery_sign_in(recovery_code, room, hub, challenge);
    fallible board_reduce(snapshot, snapshot_frontier, items, frontier);
    fallible invite_link_parse(text);
    fallible invite_link_check(text, now_ms);
    plain invite_life_ms(role);
    plain invite_clock_tolerance_ms();
    plain check_emoji();
    fallible hub_address(text);
    fallible kit_keys_for(name, words);
    fallible account_id_parse(text);
    fallible envelope_header(envelope);
    fallible body_from_wire(kind_name, payload);
    fallible bodies_from_wire(kind_names, payloads);
    fallible body_to_wire(kind_name, fields_json);
    plain board_item_parse(payload);
    fallible register_to_wire(key, value_json);
    fallible register_from_wire(name, value_json);
    plain register_key_of(name);
    plain register_name_of(key);
    fallible body_file_ids(kind_name, fields_json);
    fallible attachment_from_wire(attachment_json);
    fallible attachment_to_wire(attachment_json);
    fallible board_entry_from_wire(entry_json, with_id);
    plain shape_id_from_wire(id);
    plain shape_id_to_wire(id);
    plain piece_from_wire(piece);
    plain off_pile(running, put_off, closed, full);
    fallible piece_to_wire(piece_json);
}

/// The methods of a facade object that return a `Result`, by name and arguments.
macro_rules! methods {
    ($object:ident { $($name:ident($($argument:ident),*);)* }) => {
        #[wasm_bindgen]
        impl $object {
            $(
                pub fn $name(&self, $($argument: &JsValue),*) -> Answer {
                    answer(attempt(|| self.0.$name($(arg($argument)?),*)))
                }
            )*
        }
    };
}

/// A new object of the facade as an answer.
fn made<T: Into<JsValue>>(result: Result<T, CoreError>) -> Answer {
    match result {
        Ok(object) => pair(&JsValue::NULL, &object.into()),
        Err(error) => refusal(&error),
    }
}

/// The facade's device.
#[wasm_bindgen]
pub struct RawDevice(CoreDevice);

#[wasm_bindgen]
impl RawDevice {
    /// A new device over what the page loaded from its store, which must be empty.
    pub fn create(loaded: &JsValue) -> Answer {
        made(
            arg(loaded)
                .and_then(CoreDevice::create_loaded)
                .map(RawDevice),
        )
    }

    /// The device that what the page loaded from its store holds.
    pub fn open(loaded: &JsValue) -> Answer {
        made(arg(loaded).and_then(CoreDevice::open_loaded).map(RawDevice))
    }

    /// The writes the calls since the last `take_writes` made, oldest first.
    pub fn take_writes(&self) -> Answer {
        answer(Ok(self.0.take_writes()))
    }

    /// Closes the device and wipes what it holds.
    pub fn close(&self) {
        self.0.close();
    }
}

methods!(RawDevice {
    id();
    room();
    cursor();
    is_human();
    is_owner();
    room_roles();
    groups();
    group(group);
    holds_key(group, epoch);
    notify_keys();
    outbox();
    outbox_accepted(id, change);
    outbox_refused(id, code);
    key_packages_to_upload(unused_at_hub, now_ms);
    key_package(now_ms);
    found_room(recovery_code, now_ms);
    found_session(agent, key_packages, now_ms);
    found_helper(parent, key_packages, now_ms);
    add_to_session(group, device, key_package, now_ms);
    remove_agents(remove, now_ms);
    remove_human_devices(cuts, now_ms);
    clean_session(group, cuts, replacement, now_ms);
    readmit_human(group, device, key_package, now_ms);
    readmit_helper(group, old, device, key_package, now_ms);
    update(group, forced, now_ms);
    archive(group);
    join_welcome(welcome, room, committer, now_ms);
    observe_room(group_info, expected_state);
    observe_session(group_info);
    process_log_entry(entry, now_ms);
    feed(items, now_ms);
    send_handover(group, recipient);
    handovers_sent();
    handover_read(group, recipient);
    key_requests(now_ms);
    answer_key_request(group, recipient, now_ms);
    send_stroke_piece(board, piece);
    send_work_trail(group, turn, number, step, now_ms);
    holds_recovery_mac();
    key_is_confirmed(group, epoch);
    send_recovery_auth(recipient);
    post_sealed_key(group, group_info, listed);
    verify_founding(group, served);
    join_room_with_code(recovery_code, served, now_ms);
    join_session_with_code(recovery_code, served, now_ms);
    new_recovery_code(recovery_code);
    replace_code(recovery_code, account, now_ms);
    prepare_recovery(recovery_code, served);
    recover(recovery_code, served, chains, account, now_ms);
    learn_history(group, founding, commits);
    group_past(group);
    learn_start(group, founding);
    learn_slice(group, commits);
    learn_finish(group);
    learn_abandon(group);
    code_check_start(recovery_code, served);
    code_check_slice(commits);
    code_check_defer(groups);
    join_room_checked(recovery_code, served, now_ms);
    recover_checked(recovery_code, served, chains, account, now_ms);
    session_check_start(founding);
    session_check_slice(group, commits);
    join_session_checked(recovery_code, group, current, now_ms);
    join_session_checked_keys(recovery_code, group, current, rows, links, now_ms);
    recovery_plan_start(recovery_code, served);
    recovery_plan_slice(commits);
    recovery_plan_finish(recovery_code, served);
    invite_open(role, session_id, app, hub, now_ms);
    invite_accept(invite_id, request, now_ms);
    invite_confirm(invite_id, code, request_hash, matches, now_ms);
    invite_recommit(invite_id, now_ms);
    invite_steps();
    invite_handover(invite_id);
    invite_checked(invite_id, helpers);
    invite_forget(invite_id);
    join_link(link, now_ms);
    join_request(link, offer, now_ms);
    join_reveal(reveal);
    join_observe(group_info);
    join_invited(welcome, now_ms);
    hub_sign_in(hub, challenge);
    seal(draft, recipient, file_ids, now_ms);
    outbox_voided(id);
    envelope_abandon(id);
    receive_envelope(envelope, change, ordered, void_code, now_ms);
    receive_relay(group, message, now_ms);
    heads_due(group, now_ms);
    compare_heads(group, writer);
    cut_of(group, device);
    chain_head(group, sender);
    chain_cut(group, device);
    object(group, object_id);
    objects(group);
    object_owner(group, object_id);
    register(group, name);
    register_of(group, name, sender);
    board_load(board, served);
    command(envelope_hash, now_ms);
    command_finished(envelope_hash);
    commands_pending();
    commands_uncertain();
    findings();
    findings_read();
});

/// The facade's file encryptor.
#[wasm_bindgen]
pub struct RawFileEncryptor(FileEncryptor);

#[wasm_bindgen]
impl RawFileEncryptor {
    /// Starts a new file.
    pub fn create() -> Answer {
        made(FileEncryptor::new().map(RawFileEncryptor))
    }
}

methods!(RawFileEncryptor {
    file_id();
    update(plaintext);
    finish();
});

/// The client core's Desk (EXPERIMENT).
#[wasm_bindgen]
pub struct RawClientDesk(ClientDesk);

#[wasm_bindgen]
impl RawClientDesk {
    /// An empty Desk.
    pub fn create() -> Answer {
        made(Ok::<_, CoreError>(RawClientDesk(ClientDesk::new())))
    }
}

methods!(RawClientDesk {
    put_cards(cards);
    put_requests(requests);
    remove(object_ids);
    clear();
    set_snooze(card, value, at);
    set_archived(session_ids);
    set_listing(at, listed);
    forget_listing();
    set_hold_all(hold);
    project(now_ms);
    put_cards_json(cards);
    put_requests_json(requests);
    project_json(now_ms);
});

/// The client core's board model.
#[wasm_bindgen]
pub struct RawClientModel(ClientModel);

#[wasm_bindgen]
impl RawClientModel {
    /// An empty model of the device `me` (hex; "" while not known).
    pub fn create(me: &JsValue) -> Answer {
        made(arg::<String>(me).map(|me| RawClientModel(ClientModel::new(me))))
    }
}

methods!(RawClientModel {
    set_me(my_device_id);
    load(snapshot);
    apply_envelopes(received, now_ms);
    apply_card_deeds(received);
    apply_received_json(received, now_ms);
    set_windows_open(open);
    set_lean_cards(lean);
    card_json(object_id);
    apply_groups(groups, roles);
    apply_commit(facts, by_recovery, removed_me, now_ms);
    apply_presence(reports, now_ms);
    stream_opened();
    apply_presence_event(event, now_ms);
    apply_here(now_ms);
    seat_plan(known, now_ms);
    seat_after_hub(candidates, hub_seated);
    seat_founded(agent);
    seat_failed(agent, now_ms);
    set_seat_grace(ms);
    desks_deleting();
    desk_contents(desk_id, board);
    desk_delete_plan(desk_id, known, by, now_ms);
    starred_cards();
    self_join_asks(answered, pending);
    self_join_verdict(session_id, nonce, request_hash);
    self_join_retire();
    join_rows(invites);
    apply_work_trail(session_id, from, turn, number, time, payload, change_number);
    apply_stroke_piece(from, board, payload, now_ms);
    echo_item(local_id, timeline_key, content, recipient, object_id, now_ms);
    echo_registers(local_id, values);
    echo_answer(local_id, object_id, answer, object_state);
    echo_note(local_id, object_id, fields, closed);
    confirm_echo(local_id, hash, seq, object_id);
    rollback_echo(local_id);
    set_desk_listing(at, listed);
    forget_desk_listing();
    set_desk_hold(hold);
    set_window(timeline_key, open, loaded_down_to, has_more);
    apply_page(timeline_key, received, scratch, now_ms);
    put_items(timeline_key, items, loaded_down_to, has_more);
    push_alert(code, message, envelope_number, sender_device_id, now_ms);
    note_newer(what, envelope_number);
    finish(now_ms);
    finish_part(now_ms, max_cards);
    snapshot();
    session_list(reading);
    next_open_after(card_id, within);
    set_reading(reading);
    touch_all();
    apply_card_deeds_json(received);
    apply_groups_json(groups, roles);
    apply_commit_json(facts, by_recovery, removed_me, now_ms);
    apply_page_json(timeline_key, received, scratch, now_ms);
});

/// The facade's file decryptor.
#[wasm_bindgen]
pub struct RawFileDecryptor(FileDecryptor);

#[wasm_bindgen]
impl RawFileDecryptor {
    /// Starts reading the file `file` names.
    pub fn create(file: &JsValue) -> Answer {
        made(arg(file).and_then(FileDecryptor::new).map(RawFileDecryptor))
    }
}

methods!(RawFileDecryptor {
    update(stored);
    finish();
});
