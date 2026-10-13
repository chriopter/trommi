//! His starred cards (the register `star/<card>`, `{ at }`: when he set the star), as every app shows them beside the
//! note: the cards that are on the board, the last starred first, then by id. One rule for web and iPhone; the apps
//! only draw.
use crate::model::Model;
use serde::Serialize;
use serde_json::Value;

/// One starred card.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Starred {
    pub object_id: String,
    /// When he set the star (ms); 0 when the register names no time.
    pub at: u64,
}

impl Model {
    /// His starred cards, the last starred first. A star whose card this device does not hold is left out; a star
    /// taken away (`null`) is none.
    pub fn starred_cards(&self) -> Vec<Starred> {
        let mut out: Vec<Starred> = self
            .human
            .iter()
            .filter_map(|(k, r)| k.strip_prefix("star/").map(|id| (id, &r.value)))
            .filter(|(id, v)| !v.is_null() && *v != &Value::Bool(false) && self.cards.contains_key(*id))
            .map(|(id, v)| Starred {
                object_id: id.to_string(),
                at: v.get("at").and_then(Value::as_u64).unwrap_or(0),
            })
            .collect();
        out.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.object_id.cmp(&b.object_id)));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::types::{Card, HumanRegister};
    use serde_json::json;

    fn model() -> Model {
        let mut m = Model::new();
        for id in ["c1", "c2", "c3", "c4"] {
            m.cards.insert(
                id.into(),
                Card {
                    object_id: id.into(),
                    ..Default::default()
                },
            );
        }
        let mut star = |id: &str, v: Value| {
            m.human.insert(
                format!("star/{id}"),
                HumanRegister {
                    value: v,
                    ..Default::default()
                },
            );
        };
        star("c1", json!({ "at": 100 }));
        star("c2", json!({ "at": 300 }));
        star("c3", Value::Null);
        star("c4", json!({ "at": 300 }));
        star("gone", json!({ "at": 999 }));
        m
    }

    #[test]
    fn the_last_starred_first_ties_by_id_none_taken_away_or_unknown() {
        let ids: Vec<String> = model().starred_cards().into_iter().map(|s| s.object_id).collect();
        assert_eq!(ids, vec!["c2", "c4", "c1"]);
    }

    #[test]
    fn a_star_without_a_time_counts_last() {
        let mut m = model();
        m.human.insert(
            "star/c3".into(),
            HumanRegister {
                value: json!(true),
                ..Default::default()
            },
        );
        let s = m.starred_cards();
        assert_eq!(s.last().map(|x| (x.object_id.as_str(), x.at)), Some(("c3", 0)));
    }
}
