// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: OVH SAS

//! Config sections that cannot break the rest of the config.
//!
//! The auth config is loaded as one document. If any field fails to
//! deserialize, the whole load fails, and a failed load makes the primary
//! model fall back to the built-in default. An optional feature must not be
//! able to cause that with a mistake in its own section.
//!
//! [`Lenient<T>`] wraps such a section. Loading keeps the section as the JSON
//! text it was written as, exactly as an unknown key would be skipped, and
//! only turns it into a `T` when the feature asks for it. A section that does
//! not describe a `T`, or that the file gives more than once, is then an
//! error for that feature alone, which is expected to refuse to run with it.
//! Saving writes the text back unchanged, so `shai auth` does not erase a
//! section it does not understand.
//!
//! Only a JSON syntax error still stops the whole file from loading, as it
//! does anywhere else in it.

use super::section::RawSection;
use serde::de::DeserializeOwned;
use std::marker::PhantomData;

/// An optional config section of type `T`, read leniently.
pub struct Lenient<T> {
    raw: RawSection,
    section: PhantomData<fn() -> T>,
}

/// A section that is present in the config but cannot be used.
///
/// Carries no detail on purpose: the parser's own message may quote the
/// offending value, which is unvalidated user input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InvalidSection;

impl<T> Lenient<T> {
    /// A section that is absent.
    pub fn unset() -> Self {
        Self {
            raw: RawSection::default(),
            section: PhantomData,
        }
    }

    /// True when the section is absent (or `null`): it is then left out when
    /// the config is saved.
    pub fn is_unset(&self) -> bool {
        self.raw.is_unset()
    }

    /// The section as found in the file.
    pub(crate) fn section(&self) -> &RawSection {
        &self.raw
    }

    pub(crate) fn section_mut(&mut self) -> &mut RawSection {
        &mut self.raw
    }
}

impl<T: DeserializeOwned> Lenient<T> {
    /// The section, or `None` when it is absent.
    ///
    /// A section is one JSON object. Anything else is invalid, including an
    /// array whose items happen to line up with the fields of `T`, and a
    /// section that is written more than once.
    pub fn get(&self) -> Result<Option<T>, InvalidSection> {
        match self.raw.text() {
            Ok(None) => Ok(None),
            Ok(Some(text)) if text.starts_with('{') => serde_json::from_str(text)
                .map(Some)
                .map_err(|_| InvalidSection),
            Ok(Some(_)) | Err(_) => Err(InvalidSection),
        }
    }
}

impl<T> Default for Lenient<T> {
    fn default() -> Self {
        Self::unset()
    }
}

impl<T> Clone for Lenient<T> {
    fn clone(&self) -> Self {
        Self {
            raw: self.raw.clone(),
            section: PhantomData,
        }
    }
}

impl<T> PartialEq for Lenient<T> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

// Nothing of the section is printed: it has not been validated, and may hold
// anything, credentials included.
impl<T> std::fmt::Debug for Lenient<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.is_unset() {
            "Lenient(unset)"
        } else {
            "Lenient(set)"
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;
    use serde_json::value::RawValue;

    #[derive(Debug, Clone, PartialEq, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Section {
        name: String,
        #[serde(default)]
        count: u32,
    }

    /// A section as the config reader builds it: one value per time the key
    /// appears in the file.
    fn section(values: &[&str]) -> Lenient<Section> {
        let mut section = Lenient::unset();
        for value in values {
            section
                .section_mut()
                .push(RawValue::from_string(value.to_string()).unwrap());
        }
        section
    }

    #[test]
    fn absent_or_null_section_is_unset() {
        for values in [&[][..], &["null"][..]] {
            let section = section(values);
            assert!(section.is_unset());
            assert_eq!(section.get(), Ok(None));
        }
        assert!(Lenient::<Section>::default().is_unset());
    }

    #[test]
    fn valid_section_is_read_when_asked_for() {
        let section = section(&[r#"{"name": "a", "count": 2}"#]);
        assert!(!section.is_unset());
        assert_eq!(
            section.get(),
            Ok(Some(Section {
                name: "a".to_string(),
                count: 2,
            }))
        );
        assert_eq!(section.clone(), section);
    }

    #[test]
    fn a_section_that_is_not_one_valid_object_is_invalid() {
        let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
        for value in [
            r#"5"#,
            r#""text""#,
            r#"[1, 2]"#,
            // Would fit the fields positionally; a section is an object.
            r#"["a", 2]"#,
            r#"{"count": 2}"#,
            r#"{"name": 7}"#,
            r#"{"name": "a", "typo": true}"#,
            r#"{"name": "a", "count": 1e999}"#,
            r#"{"name": "\ud800"}"#,
            deep.as_str(),
        ] {
            let section = section(&[value]);
            assert!(!section.is_unset(), "{value}");
            assert_eq!(section.get(), Err(InvalidSection), "{value}");
        }
    }

    #[test]
    fn a_section_given_twice_is_invalid_even_if_each_copy_is_fine() {
        let valid = r#"{"name": "a"}"#;
        for values in [[valid, valid], [valid, "null"], ["null", valid]] {
            let section = section(&values);
            assert!(!section.is_unset(), "{values:?}");
            assert_eq!(section.get(), Err(InvalidSection), "{values:?}");
        }
    }

    #[test]
    fn debug_does_not_print_the_section() {
        let section = section(&[r#"{"name": 7, "api_key": "sk-secret-value"}"#]);
        assert_eq!(format!("{:?}", section), "Lenient(set)");
    }
}
