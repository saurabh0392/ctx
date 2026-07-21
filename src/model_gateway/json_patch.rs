//! Surgical JSON string replacement for prompt-cache-friendly model requests.
//!
//! The adapter supplies a parsed path and expected text. We locate a unique encoded literal in the
//! original bytes, replace only that span, then parse and compare against an independently updated
//! JSON value. Ambiguity or any unrelated semantic change fails closed.

use std::fmt;

use serde_json::Value;

use super::canonical::JsonPathSegment;

pub(super) struct TextLeafReplacement<'a> {
    pub path: &'a [JsonPathSegment],
    pub expected: &'a str,
    pub replacement: &'a str,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum JsonPatchError {
    InvalidJson,
    EmptyPath,
    DuplicatePath,
    PathMissing,
    StaleExpectedText,
    AmbiguousEncodedText,
    OverlappingSpans,
    PatchedJsonInvalid,
    UnrelatedValueChanged,
}

impl JsonPatchError {
    pub(super) fn code(self) -> &'static str {
        match self {
            Self::InvalidJson => "patch-invalid-json",
            Self::EmptyPath => "patch-empty-path",
            Self::DuplicatePath => "patch-duplicate-path",
            Self::PathMissing => "patch-path-missing",
            Self::StaleExpectedText => "patch-stale-expected-text",
            Self::AmbiguousEncodedText => "patch-ambiguous-encoded-text",
            Self::OverlappingSpans => "patch-overlapping-spans",
            Self::PatchedJsonInvalid => "patch-produced-invalid-json",
            Self::UnrelatedValueChanged => "patch-unrelated-value-changed",
        }
    }
}

impl fmt::Display for JsonPatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for JsonPatchError {}

pub(super) fn patch_text_leaves(
    body: &[u8],
    replacements: &[TextLeafReplacement<'_>],
) -> Result<Vec<u8>, JsonPatchError> {
    if replacements.is_empty() {
        return Ok(body.to_vec());
    }
    let original: Value = serde_json::from_slice(body).map_err(|_| JsonPatchError::InvalidJson)?;
    let mut expected_value = original.clone();
    let mut spans = Vec::with_capacity(replacements.len());

    for (index, replacement) in replacements.iter().enumerate() {
        if replacement.path.is_empty() {
            return Err(JsonPatchError::EmptyPath);
        }
        if replacements[..index]
            .iter()
            .any(|prior| prior.path == replacement.path)
        {
            return Err(JsonPatchError::DuplicatePath);
        }
        let source = value_at(&original, replacement.path).ok_or(JsonPatchError::PathMissing)?;
        if source.as_str() != Some(replacement.expected) {
            return Err(JsonPatchError::StaleExpectedText);
        }
        let encoded =
            serde_json::to_vec(replacement.expected).map_err(|_| JsonPatchError::InvalidJson)?;
        let matches = find_all(body, &encoded);
        if matches.len() != 1 {
            return Err(JsonPatchError::AmbiguousEncodedText);
        }
        let start = matches[0];
        spans.push((
            start,
            start + encoded.len(),
            serde_json::to_vec(replacement.replacement).map_err(|_| JsonPatchError::InvalidJson)?,
        ));
        *value_at_mut(&mut expected_value, replacement.path)
            .ok_or(JsonPatchError::PathMissing)? = Value::String(replacement.replacement.into());
    }

    spans.sort_by_key(|span| span.0);
    if spans.windows(2).any(|pair| pair[0].1 > pair[1].0) {
        return Err(JsonPatchError::OverlappingSpans);
    }
    let mut patched = body.to_vec();
    for (start, end, replacement) in spans.into_iter().rev() {
        patched.splice(start..end, replacement);
    }
    let parsed: Value =
        serde_json::from_slice(&patched).map_err(|_| JsonPatchError::PatchedJsonInvalid)?;
    if parsed != expected_value {
        return Err(JsonPatchError::UnrelatedValueChanged);
    }
    Ok(patched)
}

fn value_at<'a>(mut value: &'a Value, path: &[JsonPathSegment]) -> Option<&'a Value> {
    for segment in path {
        value = match segment {
            JsonPathSegment::Field(field) => value.as_object()?.get(*field)?,
            JsonPathSegment::Index(index) => value.as_array()?.get(*index)?,
        };
    }
    Some(value)
}

fn value_at_mut<'a>(mut value: &'a mut Value, path: &[JsonPathSegment]) -> Option<&'a mut Value> {
    for segment in path {
        value = match segment {
            JsonPathSegment::Field(field) => value.as_object_mut()?.get_mut(*field)?,
            JsonPathSegment::Index(index) => value.as_array_mut()?.get_mut(*index)?,
        };
    }
    Some(value)
}

fn find_all(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    haystack
        .windows(needle.len())
        .enumerate()
        .filter_map(|(index, candidate)| (candidate == needle).then_some(index))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path() -> Vec<JsonPathSegment> {
        vec![
            JsonPathSegment::Field("input"),
            JsonPathSegment::Index(1),
            JsonPathSegment::Field("output"),
        ]
    }

    #[test]
    fn changes_only_the_exact_json_string_literal() {
        let body = br#"{ "model": "same", "input": [ {"type":"message","content":"keep"}, { "type":"function_call_output", "output": "line one\nline two", "vendor": {"same":true} } ], "unknown": 7 }"#;
        let original_literal = serde_json::to_vec("line one\nline two").unwrap();
        let start = find_all(body, &original_literal)[0];
        let end = start + original_literal.len();
        let patched = patch_text_leaves(
            body,
            &[TextLeafReplacement {
                path: &path(),
                expected: "line one\nline two",
                replacement: "line one",
            }],
        )
        .unwrap();
        let replacement_literal = serde_json::to_vec("line one").unwrap();
        assert_eq!(&patched[..start], &body[..start]);
        assert_eq!(&patched[start + replacement_literal.len()..], &body[end..]);
        assert_eq!(
            patch_text_leaves(
                body,
                &[TextLeafReplacement {
                    path: &path(),
                    expected: "line one\nline two",
                    replacement: "line one",
                }]
            )
            .unwrap(),
            patched,
            "replay must be deterministic"
        );
    }

    #[test]
    fn ambiguous_or_stale_text_fails_closed() {
        let duplicate = br#"{"input":[{"output":"same"},{"output":"same"}]}"#;
        let second = vec![
            JsonPathSegment::Field("input"),
            JsonPathSegment::Index(1),
            JsonPathSegment::Field("output"),
        ];
        assert_eq!(
            patch_text_leaves(
                duplicate,
                &[TextLeafReplacement {
                    path: &second,
                    expected: "same",
                    replacement: "short",
                }]
            ),
            Err(JsonPatchError::AmbiguousEncodedText)
        );
        assert_eq!(
            patch_text_leaves(
                br#"{"input":[{}, {"output":"actual"}]}"#,
                &[TextLeafReplacement {
                    path: &path(),
                    expected: "stale",
                    replacement: "short",
                }]
            ),
            Err(JsonPatchError::StaleExpectedText)
        );
    }
}
