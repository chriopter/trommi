//! Device activity metadata. It never changes standing or authorizes a membership change.
use crate::error::Res;
use crate::observer::Device;
use crate::store::Room;
use rusqlite::{params, Connection};
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
