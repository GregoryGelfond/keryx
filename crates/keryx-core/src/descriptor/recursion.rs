//! Containment-cycle analysis (§8): a message is recursive if it can reach itself
//! through message-typed fields — singular, repeated, or map value. v0 translates
//! recursive schemas with path terms regardless (§8); the flag surfaces the cycle
//! for `explain` and the manifest. Pure over the built model — engine-independent.

use std::collections::{BTreeMap, BTreeSet};

use super::model::{FieldShape, Message, ValueType};

/// Mark every message that participates in a containment cycle. Deterministic;
/// linear in the containment graph per message.
pub(super) fn mark(messages: &mut [Message]) {
    let edges = containment_edges(messages);
    let recursive: BTreeSet<String> = edges
        .keys()
        .filter(|start| reaches_self(start, &edges))
        .cloned()
        .collect();
    for message in messages {
        message.recursive = recursive.contains(message.path.as_str());
    }
}

fn containment_edges(messages: &[Message]) -> BTreeMap<String, BTreeSet<String>> {
    let mut edges: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for message in messages {
        let targets = edges.entry(message.path.as_str().to_owned()).or_default();
        for field in &message.fields {
            let value = match &field.shape {
                FieldShape::Singular { value, .. }
                | FieldShape::Repeated { value }
                | FieldShape::Map { value, .. } => value,
            };
            if let ValueType::Message(name) = value {
                targets.insert(name.as_str().to_owned());
            }
        }
    }
    edges
}

fn reaches_self(start: &str, edges: &BTreeMap<String, BTreeSet<String>>) -> bool {
    let mut stack: Vec<&str> = edges
        .get(start)
        .into_iter()
        .flatten()
        .map(String::as_str)
        .collect();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    while let Some(node) = stack.pop() {
        if node == start {
            return true;
        }
        if seen.insert(node)
            && let Some(next) = edges.get(node)
        {
            stack.extend(next.iter().map(String::as_str));
        }
    }
    false
}
