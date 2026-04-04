// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

pub mod calls;
pub mod implementations;
pub mod imports;
pub mod inheritance;
pub mod type_references;

pub use calls::CallRelation;
pub use implementations::ImplementationRelation;
pub use imports::ImportRelation;
pub use inheritance::InheritanceRelation;
pub use type_references::TypeReference;
