// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: OVH SAS

//! An isolated client must not read the real process environment.
//!
//! The unit tests check this against a per-thread stand-in for the
//! environment. This file checks it against the real thing, which is
//! process-wide: it is therefore an integration test, built as its own
//! binary, and holds a single test so that nothing else in the process can
//! observe the variables it sets. No request is sent: clients are only
//! constructed.

use shai_llm::LlmClient;
use std::collections::HashMap;

#[test]
fn isolated_clients_ignore_the_real_process_environment() {
    for (name, value) in [
        ("OPENAI_API_KEY", "env-openai"),
        ("ANTHROPIC_API_KEY", "env-anthropic"),
        ("MISTRAL_API_KEY", "env-mistral"),
        ("OPENROUTER_API_KEY", "env-openrouter"),
        ("OPENAI_COMPATIBLE_API_KEY", "env-compatible"),
        ("OPENAI_COMPATIBLE_BASE_URL", "http://env.invalid/v1"),
        ("OVH_API_KEY", "env-ovh"),
        ("OVH_BASE_URL", "http://env.invalid/v1"),
        ("OLLAMA_API_KEY", "env-ollama"),
        ("OLLAMA_BASE_URL", "http://env.invalid/v1"),
    ] {
        std::env::set_var(name, value);
    }

    let empty = HashMap::new();
    for provider in [
        "openai",
        "anthropic",
        "mistral",
        "openrouter",
        "openai_compatible",
        "ovhcloud",
        "ollama",
    ] {
        // The environment is enough for the primary path...
        assert!(
            LlmClient::create_provider(provider, &empty).is_ok(),
            "{provider} should be built from the environment"
        );
        // ...and never completes an entry for the isolated one.
        let error = LlmClient::create_provider_isolated(provider, &empty)
            .expect_err("an empty entry must not be completed from the environment")
            .to_string();
        assert!(
            error.contains("not found in the provider's config entry"),
            "{provider}: {error}"
        );
        assert!(LlmClient::isolated_settings(provider, &empty).is_err());
    }

    // Two entries that differ only in what the environment could add are
    // still told apart by their own content alone.
    let with_key: HashMap<String, String> = [("OVH_API_KEY", "entry-key")]
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .into();
    let with_key_and_url: HashMap<String, String> = [
        ("OVH_API_KEY", "entry-key"),
        ("OVH_BASE_URL", "http://env.invalid/v1"),
    ]
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .into();
    assert_ne!(
        LlmClient::isolated_settings("ovhcloud", &with_key).unwrap(),
        LlmClient::isolated_settings("ovhcloud", &with_key_and_url).unwrap(),
    );
}
