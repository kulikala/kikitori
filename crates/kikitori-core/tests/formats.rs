use kikitori_core::{emit, Segment};

fn fixture_segments() -> Vec<Segment> {
    serde_json::from_str(include_str!("../../../tests/fixtures/format/segments.json")).unwrap()
}

#[test]
fn generated_formats_match_python_fixtures() {
    let segments = fixture_segments();
    for format in ["txt", "json", "yaml", "srt", "vtt"] {
        let actual = emit(&segments, format).unwrap();
        let expected = match format {
            "txt" => include_bytes!("../../../tests/fixtures/format/out.txt").as_slice(),
            "json" => include_bytes!("../../../tests/fixtures/format/out.json").as_slice(),
            "yaml" => include_bytes!("../../../tests/fixtures/format/out.yaml").as_slice(),
            "srt" => include_bytes!("../../../tests/fixtures/format/out.srt").as_slice(),
            _ => include_bytes!("../../../tests/fixtures/format/out.vtt").as_slice(),
        };
        assert_eq!(actual, expected, "format {format}");
    }
}

#[test]
fn empty_formats_match_python_fixtures_and_yaml_is_explicit_empty_list() {
    for format in ["txt", "json", "yaml", "srt", "vtt"] {
        let actual = emit(&[], format).unwrap();
        if format == "yaml" {
            assert_eq!(actual, b"[]\n");
            continue;
        }
        let expected: &[u8] = match format {
            "txt" => include_bytes!("../../../tests/fixtures/format/empty.txt"),
            "json" => include_bytes!("../../../tests/fixtures/format/empty.json"),
            "yaml" => unreachable!(),
            "srt" => include_bytes!("../../../tests/fixtures/format/empty.srt"),
            _ => include_bytes!("../../../tests/fixtures/format/empty.vtt"),
        };
        assert_eq!(actual, expected, "format {format}");
    }
}

#[test]
fn json_v1_wraps_segments_with_schema_version() {
    let result = String::from_utf8(emit(&[], "json-v1").unwrap()).unwrap();
    assert_eq!(result, "{\n \"schema_version\": 1,\n \"segments\": []\n}\n");
}
