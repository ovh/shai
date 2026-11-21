// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: OVH SAS

//! A machine on which the config directory cannot exist has no config file.
//!
//! This lives in its own test binary because it points the process
//! environment somewhere else, which no other test should see.

use shai_core::config::config::ShaiConfig;
use shai_core::config::roles::RoleError;

#[test]
fn no_config_directory_means_no_config_file() {
    // A regular file where the config directory would have to be created.
    let dir = tempfile::TempDir::new().unwrap();
    let blocker = dir.path().join("not-a-directory");
    std::fs::write(&blocker, "x").unwrap();
    std::env::set_var("XDG_CONFIG_HOME", blocker.join("config"));
    std::env::set_var("HOME", blocker.join("home"));

    // The directory cannot be made, so nothing can be in it. That is a
    // missing config, which asks for nothing, and not an unreadable one.
    assert!(ShaiConfig::config_path().is_err());
    assert_eq!(
        ShaiConfig::load_for_roles().unwrap_err(),
        RoleError::ConfigMissing
    );
    assert!(blocker.is_file());
}
