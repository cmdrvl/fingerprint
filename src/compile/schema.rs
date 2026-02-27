/// Generate JSON Schema for the fingerprint DSL (.fp.yaml format).
pub fn dsl_json_schema() -> String {
    let schema = serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "Fingerprint DSL Definition",
        "type": "object",
        "additionalProperties": false,
        "required": ["fingerprint_id", "format", "assertions"],
        "properties": {
            "fingerprint_id": {
                "type": "string",
                "minLength": 1,
            },
            "format": {
                "type": "string",
                "enum": ["xlsx", "csv", "pdf", "markdown", "text"],
            },
            "valid_from": {
                "type": "string",
                "pattern": "^\\d{4}-\\d{2}-\\d{2}$",
                "description": "Optional metadata-only lower bound date (YYYY-MM-DD).",
            },
            "valid_until": {
                "type": "string",
                "pattern": "^\\d{4}-\\d{2}-\\d{2}$",
                "description": "Optional metadata-only upper bound date (YYYY-MM-DD).",
            },
            "parent": {
                "type": "string",
                "minLength": 1,
            },
            "assertions": {
                "type": "array",
                "items": { "$ref": "#/$defs/namedAssertion" },
            },
            "extract": {
                "type": "array",
                "items": { "$ref": "#/$defs/extractSection" },
                "default": [],
            },
            "content_hash": { "$ref": "#/$defs/contentHashConfig" },
        },
        "$defs": {
            "namedAssertion": {
                "oneOf": [
                    { "$ref": "#/$defs/assertion_filename_regex" },
                    { "$ref": "#/$defs/assertion_sheet_exists" },
                    { "$ref": "#/$defs/assertion_sheet_name_regex" },
                    { "$ref": "#/$defs/assertion_cell_eq" },
                    { "$ref": "#/$defs/assertion_cell_regex" },
                    { "$ref": "#/$defs/assertion_range_non_null" },
                    { "$ref": "#/$defs/assertion_sheet_min_rows" },
                    { "$ref": "#/$defs/assertion_column_search" },
                    { "$ref": "#/$defs/assertion_header_row_match" },
                    { "$ref": "#/$defs/assertion_range_populated" },
                    { "$ref": "#/$defs/assertion_sum_eq" },
                    { "$ref": "#/$defs/assertion_within_tolerance" },
                    { "$ref": "#/$defs/assertion_heading_exists" },
                    { "$ref": "#/$defs/assertion_heading_regex" },
                    { "$ref": "#/$defs/assertion_heading_level" },
                    { "$ref": "#/$defs/assertion_text_contains" },
                    { "$ref": "#/$defs/assertion_text_regex" },
                    { "$ref": "#/$defs/assertion_text_near" },
                    { "$ref": "#/$defs/assertion_section_non_empty" },
                    { "$ref": "#/$defs/assertion_section_min_lines" },
                    { "$ref": "#/$defs/assertion_table_exists" },
                    { "$ref": "#/$defs/assertion_table_columns" },
                    { "$ref": "#/$defs/assertion_table_shape" },
                    { "$ref": "#/$defs/assertion_table_min_rows" },
                    { "$ref": "#/$defs/assertion_page_count" },
                    { "$ref": "#/$defs/assertion_metadata_regex" },
                ],
            },
            "assertion_filename_regex": {
                "type": "object",
                "additionalProperties": false,
                "required": ["filename_regex"],
                "properties": {
                    "name": { "type": "string" },
                    "filename_regex": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["pattern"],
                        "properties": {
                            "pattern": { "type": "string", "minLength": 1 },
                        },
                    },
                },
            },
            "assertion_sheet_exists": {
                "type": "object",
                "additionalProperties": false,
                "required": ["sheet_exists"],
                "properties": {
                    "name": { "type": "string" },
                    "sheet_exists": { "type": "string", "minLength": 1 },
                },
            },
            "assertion_sheet_name_regex": {
                "type": "object",
                "additionalProperties": false,
                "required": ["sheet_name_regex"],
                "properties": {
                    "name": { "type": "string" },
                    "sheet_name_regex": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["pattern"],
                        "properties": {
                            "pattern": { "type": "string", "minLength": 1 },
                            "bind": { "type": "string", "minLength": 1 },
                        },
                    },
                },
            },
            "assertion_cell_eq": {
                "type": "object",
                "additionalProperties": false,
                "required": ["cell_eq"],
                "properties": {
                    "name": { "type": "string" },
                    "cell_eq": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["sheet", "cell", "value"],
                        "properties": {
                            "sheet": { "type": "string", "minLength": 1 },
                            "cell": { "type": "string", "minLength": 1 },
                            "value": { "type": "string" },
                        },
                    },
                },
            },
            "assertion_cell_regex": {
                "type": "object",
                "additionalProperties": false,
                "required": ["cell_regex"],
                "properties": {
                    "name": { "type": "string" },
                    "cell_regex": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["sheet", "cell", "pattern"],
                        "properties": {
                            "sheet": { "type": "string", "minLength": 1 },
                            "cell": { "type": "string", "minLength": 1 },
                            "pattern": { "type": "string", "minLength": 1 },
                        },
                    },
                },
            },
            "assertion_range_non_null": {
                "type": "object",
                "additionalProperties": false,
                "required": ["range_non_null"],
                "properties": {
                    "name": { "type": "string" },
                    "range_non_null": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["sheet", "range"],
                        "properties": {
                            "sheet": { "type": "string", "minLength": 1 },
                            "range": { "type": "string", "minLength": 1 },
                        },
                    },
                },
            },
            "assertion_sheet_min_rows": {
                "type": "object",
                "additionalProperties": false,
                "required": ["sheet_min_rows"],
                "properties": {
                    "name": { "type": "string" },
                    "sheet_min_rows": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["sheet", "min_rows"],
                        "properties": {
                            "sheet": { "type": "string", "minLength": 1 },
                            "min_rows": { "type": "integer", "minimum": 0 },
                        },
                    },
                },
            },
            "assertion_column_search": {
                "type": "object",
                "additionalProperties": false,
                "required": ["column_search"],
                "properties": {
                    "name": { "type": "string" },
                    "column_search": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["sheet", "column", "row_range", "pattern"],
                        "properties": {
                            "sheet": { "type": "string", "minLength": 1 },
                            "column": { "type": "string", "minLength": 1 },
                            "row_range": {
                                "type": "string",
                                "pattern": "^\\s*\\d+\\s*:\\s*\\d+\\s*$"
                            },
                            "pattern": { "type": "string", "minLength": 1 },
                        },
                    },
                },
            },
            "assertion_header_row_match": {
                "type": "object",
                "additionalProperties": false,
                "required": ["header_row_match"],
                "properties": {
                    "name": { "type": "string" },
                    "header_row_match": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["sheet", "row_range", "min_match", "columns"],
                        "properties": {
                            "sheet": { "type": "string", "minLength": 1 },
                            "row_range": {
                                "type": "string",
                                "pattern": "^\\s*\\d+\\s*:\\s*\\d+\\s*$"
                            },
                            "min_match": { "type": "integer", "minimum": 1 },
                            "columns": {
                                "type": "array",
                                "minItems": 1,
                                "items": {
                                    "type": "object",
                                    "additionalProperties": false,
                                    "required": ["pattern"],
                                    "properties": {
                                        "pattern": { "type": "string", "minLength": 1 }
                                    }
                                }
                            },
                        },
                    },
                },
            },
            "assertion_range_populated": {
                "type": "object",
                "description": "Known assertion type, unsupported in v0.1 runtime.",
                "x-runtime-support": "unsupported_in_v0_1",
                "additionalProperties": false,
                "required": ["range_populated"],
                "properties": {
                    "name": { "type": "string" },
                    "range_populated": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["sheet", "range", "min_pct"],
                        "properties": {
                            "sheet": { "type": "string", "minLength": 1 },
                            "range": { "type": "string", "minLength": 1 },
                            "min_pct": { "type": "number", "minimum": 0.0, "maximum": 1.0 },
                        },
                    },
                },
            },
            "assertion_sum_eq": {
                "type": "object",
                "description": "Known assertion type, unsupported in v0.1 runtime.",
                "x-runtime-support": "unsupported_in_v0_1",
                "additionalProperties": false,
                "required": ["sum_eq"],
                "properties": {
                    "name": { "type": "string" },
                    "sum_eq": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["range", "equals_cell", "tolerance"],
                        "properties": {
                            "range": { "type": "string", "minLength": 1 },
                            "equals_cell": { "type": "string", "minLength": 1 },
                            "tolerance": { "type": "number", "minimum": 0.0 },
                        },
                    },
                },
            },
            "assertion_within_tolerance": {
                "type": "object",
                "description": "Known assertion type, unsupported in v0.1 runtime.",
                "x-runtime-support": "unsupported_in_v0_1",
                "additionalProperties": false,
                "required": ["within_tolerance"],
                "properties": {
                    "name": { "type": "string" },
                    "within_tolerance": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["cell", "min", "max"],
                        "properties": {
                            "cell": { "type": "string", "minLength": 1 },
                            "min": { "type": "number" },
                            "max": { "type": "number" },
                        },
                    },
                },
            },
            "assertion_heading_exists": {
                "type": "object",
                "additionalProperties": false,
                "required": ["heading_exists"],
                "properties": {
                    "name": { "type": "string" },
                    "heading_exists": { "type": "string", "minLength": 1 },
                },
            },
            "assertion_heading_regex": {
                "type": "object",
                "additionalProperties": false,
                "required": ["heading_regex"],
                "properties": {
                    "name": { "type": "string" },
                    "heading_regex": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["pattern"],
                        "properties": {
                            "pattern": { "type": "string", "minLength": 1 },
                        },
                    },
                },
            },
            "assertion_heading_level": {
                "type": "object",
                "additionalProperties": false,
                "required": ["heading_level"],
                "properties": {
                    "name": { "type": "string" },
                    "heading_level": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["level", "pattern"],
                        "properties": {
                            "level": { "type": "integer", "minimum": 1, "maximum": 6 },
                            "pattern": { "type": "string", "minLength": 1 },
                        },
                    },
                },
            },
            "assertion_text_contains": {
                "type": "object",
                "additionalProperties": false,
                "required": ["text_contains"],
                "properties": {
                    "name": { "type": "string" },
                    "text_contains": { "type": "string", "minLength": 1 },
                },
            },
            "assertion_text_regex": {
                "type": "object",
                "additionalProperties": false,
                "required": ["text_regex"],
                "properties": {
                    "name": { "type": "string" },
                    "text_regex": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["pattern"],
                        "properties": {
                            "pattern": { "type": "string", "minLength": 1 },
                        },
                    },
                },
            },
            "assertion_text_near": {
                "type": "object",
                "additionalProperties": false,
                "required": ["text_near"],
                "properties": {
                    "name": { "type": "string" },
                    "text_near": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["anchor", "pattern", "within_chars"],
                        "properties": {
                            "anchor": { "type": "string", "minLength": 1 },
                            "pattern": { "type": "string", "minLength": 1 },
                            "within_chars": { "type": "integer", "minimum": 0 },
                        },
                    },
                },
            },
            "assertion_section_non_empty": {
                "type": "object",
                "additionalProperties": false,
                "required": ["section_non_empty"],
                "properties": {
                    "name": { "type": "string" },
                    "section_non_empty": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["heading"],
                        "properties": {
                            "heading": { "type": "string", "minLength": 1 },
                        },
                    },
                },
            },
            "assertion_section_min_lines": {
                "type": "object",
                "additionalProperties": false,
                "required": ["section_min_lines"],
                "properties": {
                    "name": { "type": "string" },
                    "section_min_lines": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["heading", "min_lines"],
                        "properties": {
                            "heading": { "type": "string", "minLength": 1 },
                            "min_lines": { "type": "integer", "minimum": 0 },
                        },
                    },
                },
            },
            "assertion_table_exists": {
                "type": "object",
                "additionalProperties": false,
                "required": ["table_exists"],
                "properties": {
                    "name": { "type": "string" },
                    "table_exists": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["heading"],
                        "properties": {
                            "heading": { "type": "string", "minLength": 1 },
                            "index": { "type": "integer", "minimum": 0 },
                        },
                    },
                },
            },
            "assertion_table_columns": {
                "type": "object",
                "additionalProperties": false,
                "required": ["table_columns"],
                "properties": {
                    "name": { "type": "string" },
                    "table_columns": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["heading", "patterns"],
                        "properties": {
                            "heading": { "type": "string", "minLength": 1 },
                            "index": { "type": "integer", "minimum": 0 },
                            "patterns": {
                                "type": "array",
                                "items": { "type": "string", "minLength": 1 },
                            },
                        },
                    },
                },
            },
            "assertion_table_shape": {
                "type": "object",
                "additionalProperties": false,
                "required": ["table_shape"],
                "properties": {
                    "name": { "type": "string" },
                    "table_shape": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["heading", "min_columns", "column_types"],
                        "properties": {
                            "heading": { "type": "string", "minLength": 1 },
                            "index": { "type": "integer", "minimum": 0 },
                            "min_columns": { "type": "integer", "minimum": 1 },
                            "column_types": {
                                "type": "array",
                                "items": { "type": "string", "minLength": 1 },
                            },
                        },
                    },
                },
            },
            "assertion_table_min_rows": {
                "type": "object",
                "additionalProperties": false,
                "required": ["table_min_rows"],
                "properties": {
                    "name": { "type": "string" },
                    "table_min_rows": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["heading", "min_rows"],
                        "properties": {
                            "heading": { "type": "string", "minLength": 1 },
                            "index": { "type": "integer", "minimum": 0 },
                            "min_rows": { "type": "integer", "minimum": 0 },
                        },
                    },
                },
            },
            "assertion_page_count": {
                "type": "object",
                "additionalProperties": false,
                "required": ["page_count"],
                "properties": {
                    "name": { "type": "string" },
                    "page_count": {
                        "type": "object",
                        "additionalProperties": false,
                        "properties": {
                            "min": { "type": "integer", "minimum": 0 },
                            "max": { "type": "integer", "minimum": 0 },
                        },
                        "anyOf": [
                            { "required": ["min"] },
                            { "required": ["max"] },
                        ],
                    },
                },
            },
            "assertion_metadata_regex": {
                "type": "object",
                "additionalProperties": false,
                "required": ["metadata_regex"],
                "properties": {
                    "name": { "type": "string" },
                    "metadata_regex": {
                        "type": "object",
                        "additionalProperties": false,
                        "required": ["key", "pattern"],
                        "properties": {
                            "key": { "type": "string", "minLength": 1 },
                            "pattern": { "type": "string", "minLength": 1 },
                        },
                    },
                },
            },
            "extractSection": {
                "type": "object",
                "additionalProperties": false,
                "required": ["name", "type"],
                "properties": {
                    "name": { "type": "string", "minLength": 1 },
                    "type": {
                        "type": "string",
                        "enum": ["range", "table", "section", "text_match"]
                    },
                    "anchor_heading": { "type": "string" },
                    "index": { "type": "integer", "minimum": 0 },
                    "anchor": { "type": "string" },
                    "pattern": { "type": "string" },
                    "within_chars": { "type": "integer", "minimum": 0 },
                    "sheet": { "type": "string" },
                    "range": { "type": "string" },
                },
            },
            "contentHashConfig": {
                "type": "object",
                "additionalProperties": false,
                "required": ["algorithm", "over"],
                "properties": {
                    "algorithm": {
                        "type": "string",
                        "enum": ["blake3"]
                    },
                    "over": {
                        "type": "array",
                        "items": { "type": "string", "minLength": 1 },
                    },
                },
            },
        },
    });

    serde_json::to_string_pretty(&schema).expect("schema JSON serialization should not fail")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::BTreeSet;

    #[test]
    fn schema_is_valid_json_and_includes_temporal_metadata_fields() {
        let parsed: Value =
            serde_json::from_str(&dsl_json_schema()).expect("schema should be valid JSON");
        let properties = parsed["properties"]
            .as_object()
            .expect("schema properties should be an object");

        assert_eq!(
            properties["valid_from"]["type"],
            Value::String("string".to_owned())
        );
        assert_eq!(
            properties["valid_until"]["type"],
            Value::String("string".to_owned())
        );
        assert_eq!(
            properties["valid_from"]["pattern"],
            Value::String("^\\d{4}-\\d{2}-\\d{2}$".to_owned())
        );
        assert_eq!(
            properties["valid_until"]["pattern"],
            Value::String("^\\d{4}-\\d{2}-\\d{2}$".to_owned())
        );
    }

    #[test]
    fn schema_includes_expected_top_level_required_fields() {
        let parsed: Value =
            serde_json::from_str(&dsl_json_schema()).expect("schema should be valid JSON");
        let required = parsed["required"]
            .as_array()
            .expect("required should be an array");

        assert!(required.contains(&Value::String("fingerprint_id".to_owned())));
        assert!(required.contains(&Value::String("format".to_owned())));
        assert!(required.contains(&Value::String("assertions".to_owned())));
    }

    #[test]
    fn schema_contains_v0_1_assertion_subschemas() {
        let parsed: Value =
            serde_json::from_str(&dsl_json_schema()).expect("schema should be valid JSON");
        let defs = parsed["$defs"]
            .as_object()
            .expect("$defs should be an object");

        for key in [
            "assertion_sheet_exists",
            "assertion_cell_eq",
            "assertion_cell_regex",
            "assertion_range_non_null",
            "assertion_sheet_min_rows",
            "assertion_column_search",
            "assertion_header_row_match",
            "assertion_filename_regex",
            "assertion_sheet_name_regex",
        ] {
            assert!(defs.contains_key(key), "missing definition: {key}");
        }
    }

    #[test]
    fn schema_marks_deferred_assertions_as_unsupported_in_v0_1() {
        let parsed: Value =
            serde_json::from_str(&dsl_json_schema()).expect("schema should be valid JSON");
        let defs = parsed["$defs"]
            .as_object()
            .expect("$defs should be an object");

        for key in [
            "assertion_range_populated",
            "assertion_sum_eq",
            "assertion_within_tolerance",
        ] {
            let description = defs[key]["description"]
                .as_str()
                .expect("deferred assertion should have description");
            assert!(description.contains("unsupported in v0.1"));
            assert_eq!(
                defs[key]["x-runtime-support"],
                Value::String("unsupported_in_v0_1".to_owned())
            );
        }
    }

    #[test]
    fn schema_output_is_deterministic() {
        assert_eq!(dsl_json_schema(), dsl_json_schema());
    }

    #[test]
    fn schema_covers_argus_example_assertion_types() {
        let sample = r#"
fingerprint_id: argus-model.v1
format: xlsx
assertions:
  - sheet_exists: "Assumptions"
  - cell_eq:
      sheet: "Assumptions"
      cell: "A3"
      value: "Market Leasing Assumptions"
  - range_non_null:
      sheet: "Assumptions"
      range: "A3:D10"
  - sheet_min_rows:
      sheet: "Rent Roll"
      min_rows: 10
extract:
  - name: market_leasing_assumptions
    type: range
    sheet: "Assumptions"
    range: "A3:D10"
content_hash:
  algorithm: blake3
  over: [market_leasing_assumptions]
"#;

        let parsed_schema: Value =
            serde_json::from_str(&dsl_json_schema()).expect("schema should be valid JSON");
        let supported_assertions = named_assertion_keys(&parsed_schema);

        let sample_yaml: serde_yaml::Value =
            serde_yaml::from_str(sample).expect("sample yaml should parse");
        let sample_json =
            serde_json::to_value(sample_yaml).expect("yaml value should convert to json value");
        let assertions = sample_json["assertions"]
            .as_array()
            .expect("sample assertions should be an array");

        for assertion in assertions {
            let object = assertion
                .as_object()
                .expect("sample assertion should be an object");
            let assertion_key = object
                .keys()
                .find(|key| key.as_str() != "name")
                .expect("assertion should contain an assertion key");
            assert!(
                supported_assertions.contains(assertion_key),
                "schema missing assertion key '{}'",
                assertion_key
            );
        }
    }

    fn named_assertion_keys(schema: &Value) -> BTreeSet<String> {
        let defs = schema["$defs"]
            .as_object()
            .expect("$defs should be an object");
        let one_of = defs["namedAssertion"]["oneOf"]
            .as_array()
            .expect("namedAssertion.oneOf should be an array");

        let mut keys = BTreeSet::new();
        for reference in one_of {
            let reference_path = reference["$ref"]
                .as_str()
                .expect("each oneOf entry should contain $ref");
            let definition_name = reference_path
                .rsplit('/')
                .next()
                .expect("ref path should include definition name");
            let properties = defs[definition_name]["properties"]
                .as_object()
                .expect("assertion definition should have object properties");

            for key in properties.keys() {
                if key != "name" {
                    keys.insert(key.clone());
                }
            }
        }

        keys
    }
}
