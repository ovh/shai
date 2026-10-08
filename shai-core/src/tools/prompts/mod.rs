// SPDX-License-Identifier: Apache-2.0
// SPDX-FileCopyrightText: OVH SAS

pub mod discovery;

pub use discovery::PromptInfo;
pub use discovery::{
    discover_prompts, load_active_prompts, load_active_prompts_from_disk, load_prompt_body,
    save_active_prompts, strip_frontmatter,
};
