// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

pub mod class;
pub mod function;
pub mod module;
pub mod trait_;

pub use class::{ClassEntity, Field};
pub use function::{truncate_body_prefix, FunctionEntity, Parameter, BODY_PREFIX_MAX_CHARS};
pub use module::ModuleEntity;
pub use trait_::TraitEntity;
