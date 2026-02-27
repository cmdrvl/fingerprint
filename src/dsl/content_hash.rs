use blake3::Hasher;
use serde_json::Value;
use std::collections::BTreeMap;
use std::collections::HashMap;

/// Compute BLAKE3 content hash over extracted content sections.
pub fn content_hash(extracted: &HashMap<String, Value>, over: &[String]) -> String {
    let mut hasher = Hasher::new();

    let selected_names: Vec<String> = if over.is_empty() {
        extracted.keys().cloned().collect::<Vec<_>>()
    } else {
        over.to_vec()
    };

    if over.is_empty() {
        let mut sorted_names = selected_names;
        sorted_names.sort();
        hash_selected_sections(&mut hasher, extracted, &sorted_names);
    } else {
        hash_selected_sections(&mut hasher, extracted, &selected_names);
    }

    format!("blake3:{}", hasher.finalize().to_hex())
}

fn hash_selected_sections(
    hasher: &mut Hasher,
    extracted: &HashMap<String, Value>,
    names: &[String],
) {
    for name in names {
        hasher.update(name.as_bytes());
        hasher.update(&[0_u8]);

        match extracted.get(name) {
            Some(value) => {
                hasher.update(&[1_u8]);
                let canonical = canonicalize_value(value);
                let encoded = serde_json::to_vec(&canonical)
                    .expect("canonicalized JSON values should always serialize");
                hasher.update(&(encoded.len() as u64).to_le_bytes());
                hasher.update(&encoded);
            }
            None => {
                hasher.update(&[2_u8]);
            }
        }

        hasher.update(&[0xff_u8]);
    }
}

fn canonicalize_value(value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(canonicalize_value).collect()),
        Value::Object(map) => {
            let mut sorted = BTreeMap::new();
            for (key, item) in map {
                sorted.insert(key.clone(), canonicalize_value(item));
            }
            let object = sorted
                .into_iter()
                .collect::<serde_json::Map<String, Value>>();
            Value::Object(object)
        }
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn hash_is_deterministic_for_identical_content() {
        let mut first = HashMap::new();
        first.insert(
            "rent_roll_table".to_owned(),
            json!({
                "start_line": 12,
                "end_line": 20,
                "columns": ["Tenant", "SF"],
                "row_count": 8,
            }),
        );
        first.insert(
            "income_cap_section".to_owned(),
            json!({
                "start_line": 30,
                "end_line": 90,
                "heading": "Income Capitalization Approach",
            }),
        );

        let mut second = HashMap::new();
        second.insert(
            "income_cap_section".to_owned(),
            json!({
                "heading": "Income Capitalization Approach",
                "end_line": 90,
                "start_line": 30,
            }),
        );
        second.insert(
            "rent_roll_table".to_owned(),
            json!({
                "row_count": 8,
                "columns": ["Tenant", "SF"],
                "end_line": 20,
                "start_line": 12,
            }),
        );

        let over = vec![
            "rent_roll_table".to_owned(),
            "income_cap_section".to_owned(),
        ];
        let first_hash = content_hash(&first, &over);
        let second_hash = content_hash(&second, &over);

        assert_eq!(first_hash, second_hash);
        assert!(first_hash.starts_with("blake3:"));
    }

    #[test]
    fn hash_changes_when_selected_content_changes() {
        let mut first = HashMap::new();
        first.insert("as_of_date".to_owned(), json!({"matched": "June 15, 2024"}));

        let mut second = HashMap::new();
        second.insert("as_of_date".to_owned(), json!({"matched": "June 16, 2024"}));

        let over = vec!["as_of_date".to_owned()];
        assert_ne!(content_hash(&first, &over), content_hash(&second, &over));
    }

    #[test]
    fn hash_all_sections_sorts_keys_when_over_is_empty() {
        let mut extracted = HashMap::new();
        extracted.insert("b".to_owned(), json!({"v": 2}));
        extracted.insert("a".to_owned(), json!({"v": 1}));

        let first = content_hash(&extracted, &[]);
        let second = content_hash(&extracted, &[]);
        assert_eq!(first, second);
    }

    #[test]
    fn hash_includes_missing_selected_sections_deterministically() {
        let mut extracted = HashMap::new();
        extracted.insert("present".to_owned(), json!({"v": 1}));

        let with_missing = content_hash(&extracted, &["present".to_owned(), "missing".to_owned()]);
        let only_present = content_hash(&extracted, &["present".to_owned()]);
        assert_ne!(with_missing, only_present);
    }
}
