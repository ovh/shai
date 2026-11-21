// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: OVH SAS

//! Optional top-level sections of the auth config that are kept as text.
//!
//! Some sections switch on features that send a session's data to further
//! models. A mistake inside one of them has to do two things at once: leave
//! the rest of the config loading as it did when the key was unknown, and
//! keep the feature from running on a guess. [`RawSection`] holds such a
//! section as the JSON text it was written as. The text is interpreted only
//! when the feature is used, and is written back unchanged when the config is
//! saved.
//!
//! What a section cannot be is something other than text: its bytes have to
//! be valid UTF-8, or the file fails to load like any file that is not JSON.
//!
//! The same goes for a section that is written more than once. A typed field
//! would reject the whole file as having a duplicate field, and taking one of
//! the copies would be a guess. Every copy is kept instead: the section is
//! unusable until the file has a single one, and saving writes all of them
//! back.

use serde::ser::SerializeMap;
use serde_json::value::RawValue;

/// The key of a section appears more than once in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Repeated;

/// Every value the file gives for one optional top-level key, in order.
#[derive(Clone, Default)]
pub(crate) struct RawSection(Vec<Box<RawValue>>);

impl RawSection {
    /// A section holding `text`, or `None` if `text` is not JSON.
    pub(crate) fn from_text(text: String) -> Option<Self> {
        RawValue::from_string(text).ok().map(|raw| Self(vec![raw]))
    }

    /// Record one more value found for the key.
    pub(crate) fn push(&mut self, value: Box<RawValue>) {
        self.0.push(value);
    }

    /// True when the file does not have the key, or has it once as `null`.
    pub(crate) fn is_unset(&self) -> bool {
        matches!(self.text(), Ok(None))
    }

    /// The text of the section, `None` when it is unset.
    pub(crate) fn text(&self) -> Result<Option<&str>, Repeated> {
        match self.0.as_slice() {
            [] => Ok(None),
            [only] if only.get() == "null" => Ok(None),
            [only] => Ok(Some(only.get())),
            _ => Err(Repeated),
        }
    }

    /// Write the section back under `key`, as it was read: nothing when it is
    /// unset, and one entry per copy otherwise.
    pub(crate) fn write<M: SerializeMap>(
        &self,
        key: &'static str,
        map: &mut M,
    ) -> Result<(), M::Error> {
        if self.is_unset() {
            return Ok(());
        }
        for value in &self.0 {
            map.serialize_entry(key, value)?;
        }
        Ok(())
    }
}

impl PartialEq for RawSection {
    fn eq(&self, other: &Self) -> bool {
        self.0.len() == other.0.len()
            && self.0.iter().zip(&other.0).all(|(a, b)| a.get() == b.get())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(text: &str) -> Box<RawValue> {
        RawValue::from_string(text.to_string()).unwrap()
    }

    #[test]
    fn a_missing_or_null_section_is_unset() {
        let mut section = RawSection::default();
        assert!(section.is_unset());
        assert_eq!(section.text(), Ok(None));

        section.push(raw("null"));
        assert!(section.is_unset());
        assert_eq!(section.text(), Ok(None));
    }

    #[test]
    fn a_single_value_is_kept_as_written() {
        let mut section = RawSection::default();
        section.push(raw(r#"{"a":  1}"#));
        assert!(!section.is_unset());
        assert_eq!(section.text(), Ok(Some(r#"{"a":  1}"#)));
        // The text is not printable on purpose: it has not been validated.
        assert!(Some(section) == RawSection::from_text(r#"{"a":  1}"#.to_string()));
        assert!(RawSection::from_text("{".to_string()).is_none());
    }

    #[test]
    fn a_repeated_section_has_no_text_whatever_the_copies_are() {
        for copies in [
            [r#"{"a":1}"#, r#"{"a":1}"#],
            [r#"{"a":1}"#, r#"{"a":2}"#],
            ["null", r#"{"a":1}"#],
            [r#"{"a":1}"#, "null"],
            ["null", "null"],
        ] {
            let mut section = RawSection::default();
            for copy in copies {
                section.push(raw(copy));
            }
            assert_eq!(section.text(), Err(Repeated), "{copies:?}");
            assert!(!section.is_unset(), "{copies:?}");
        }
    }
}
