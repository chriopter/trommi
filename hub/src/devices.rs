//! Device activity metadata. It never changes standing or authorizes a membership change.
use crate::error::Res;
use crate::observer::Device;
use crate::store::Room;
use rusqlite::{params, Connection, OptionalExtension};
use serde_json::{json, Value};
use std::collections::BTreeMap;

/// A signed sign-in or an opened/closed stream proves contact. Coalesce repeated contacts within a minute.
pub fn touch(c: &Connection, room: &Room, device: &Device, now: u64) -> Res<()> {
    c.execute(
        "INSERT INTO device_seen(room_id, device, at)
         SELECT room_id, device, ?3 FROM devices WHERE room_id = ?1 AND device = ?2 AND removed_epoch IS NULL
         ON CONFLICT(room_id, device) DO UPDATE SET at = excluded.at WHERE device_seen.at <= excluded.at - 60000",
        params![&room[..], &device[..], now as i64],
    )?;
    Ok(())
}

/// The room's known contact times, including devices whose streams are closed.
pub fn seen(c: &Connection, room: &Room) -> Res<Vec<(Device, u64)>> {
    let mut q = c.prepare_cached("SELECT device, at FROM device_seen WHERE room_id = ?1 ORDER BY device")?;
    let rows = q.query_map([&room[..]], |r| Ok((crate::store::fixed::<32>(r.get(0)?)?, r.get::<_, i64>(1)? as u64)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// One complete presence row per device. Contact metadata, live streams and active agent leases are
/// folded before sending, so a timestamp cannot briefly mark a leased agent offline. Two queries
/// suffice regardless of the number of devices; no per-device lease lookup.
pub fn snapshot(c: &Connection, room: &Room, me: &Device, online: &[Device], now: u64) -> Res<Vec<Value>> {
    let mut rows = BTreeMap::new();
    for (device, at) in seen(c, room)? {
        rows.insert(device, json!({"device": crate::util::b64(&device), "online": false, "last_seen_ms": at}));
    }
    for device in online {
        rows.entry(*device).or_insert_with(|| json!({"device": crate::util::b64(device)}))["online"] = json!(true);
    }
    let mut leases = c.prepare_cached("SELECT device, hears, working, last_call_at FROM agent_leases WHERE room_id = ?1 AND expires_at > ?2")?;
    let held = leases.query_map(params![&room[..], now as i64], |r| Ok((crate::store::fixed::<32>(r.get(0)?)?, r.get::<_, bool>(1)?, r.get::<_, bool>(2)?, r.get::<_, i64>(3)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (device, hears, working, last_call_at) in held {
        let row = rows.entry(device).or_insert_with(|| json!({"device": crate::util::b64(&device)}));
        row["online"] = json!(true);
        row["hears"] = json!(hears);
        row["working"] = json!(working);
        row["last_call_at"] = json!(last_call_at);
    }
    rows.remove(me);
    Ok(rows.into_values().collect())
}

/// Only this exact key can request its retirement. Unknown keys may be an own sign-in Commit still
/// in flight: retain at most 1000 such requests per room for an hour. A receiver still has to verify
/// human membership and the signature itself before making the actual removal Commit.
pub fn depart(c: &Connection, observer: &dyn crate::observer::Observer, room: &Room, device: &Device, signature: &[u8], now: u64) -> Res<bool> {
    use crate::error::refuse;
    if !observer.verify(device, trommi_wire::signin::LABEL_DEVICE_DEPARTURE, &trommi_wire::signin::departure_content(room, device), signature) {
        return Err(refuse("bad-signature", "the departure must be signed by that device for this room"));
    }
    let exists: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM rooms WHERE room_id = ?1)", [&room[..]], |r| r.get(0))?;
    if !exists { return Ok(false); }
    let role: Option<(String, bool)> = c.query_row("SELECT role, removed_epoch IS NOT NULL FROM devices WHERE room_id = ?1 AND device = ?2", params![&room[..], &device[..]], |r| Ok((r.get(0)?,r.get(1)?))).optional()?;
    if role.as_ref().is_some_and(|(_, removed)| *removed) { return Ok(false); }
    if role.as_ref().is_some_and(|(role, _)| role != "human") || matches!(crate::store::standing(c, room, device)?, Some(crate::store::Who::Recovery | crate::store::Who::Spent)) {
        return Err(refuse("forbidden", "only a human device requests its own logout"));
    }
    c.execute("DELETE FROM device_departures AS p WHERE room_id = ?1 AND (EXISTS(SELECT 1 FROM devices d WHERE d.room_id = p.room_id AND d.device = p.device AND d.removed_epoch IS NOT NULL) OR (at < ?2 AND NOT EXISTS(SELECT 1 FROM devices d WHERE d.room_id = p.room_id AND d.device = p.device)))", params![&room[..],now.saturating_sub(3_600_000) as i64])?;
    if role.is_none() {
        let count: i64 = c.query_row("SELECT count(*) FROM device_departures p WHERE room_id = ?1 AND NOT EXISTS(SELECT 1 FROM devices d WHERE d.room_id = p.room_id AND d.device = p.device)", [&room[..]], |r| r.get(0))?;
        let already: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM device_departures WHERE room_id = ?1 AND device = ?2)", params![&room[..],&device[..]], |r| r.get(0))?;
        if count >= 1000 && !already { return Err(refuse("busy", "too many unfinished departure requests")); }
    }
    Ok(c.execute("INSERT INTO device_departures(room_id,device,signature,at) VALUES (?1,?2,?3,?4) ON CONFLICT(room_id,device) DO NOTHING",params![&room[..],&device[..],signature,now as i64])? != 0)
}

pub fn departures(c: &Connection, room: &Room, now: u64) -> Res<Vec<Value>> {
    let mut q = c.prepare_cached("SELECT p.device,p.signature FROM device_departures p LEFT JOIN devices d ON d.room_id=p.room_id AND d.device=p.device WHERE p.room_id=?1 AND ((d.role='human' AND d.removed_epoch IS NULL) OR (d.device IS NULL AND p.at >= ?2)) ORDER BY p.device")?;
    let rows = q.query_map(params![&room[..],now.saturating_sub(3_600_000) as i64],|r| Ok((crate::store::fixed::<32>(r.get(0)?)?,r.get::<_,Vec<u8>>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows.into_iter().map(|(device, signature)| json!({"device":crate::util::b64(&device),"signature":crate::util::b64(&signature)})).collect())
}
