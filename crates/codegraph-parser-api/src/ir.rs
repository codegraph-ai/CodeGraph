// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

use crate::{
    entities::{ClassEntity, FunctionEntity, ModuleEntity, TraitEntity},
    relationships::{
        CallRelation, ImplementationRelation, ImportRelation, InheritanceRelation, TypeReference,
    },
};
use std::path::PathBuf;

/// Intermediate representation of extracted code
///
/// This is the bridge between language-specific AST and the CodeGraph database.
/// Parsers extract entities and relationships into this IR, then the IR is
/// inserted into the graph in a batch operation.
#[derive(Debug, Default, Clone)]
pub struct CodeIR {
    /// Source file path
    pub file_path: PathBuf,

    /// Module/file entity
    pub module: Option<ModuleEntity>,

    /// Extracted functions
    pub functions: Vec<FunctionEntity>,

    /// Extracted classes
    pub classes: Vec<ClassEntity>,

    /// Extracted traits/interfaces
    pub traits: Vec<TraitEntity>,

    /// Function call relationships
    pub calls: Vec<CallRelation>,

    /// Import relationships
    pub imports: Vec<ImportRelation>,

    /// Inheritance relationships
    pub inheritance: Vec<InheritanceRelation>,

    /// Implementation relationships
    pub implementations: Vec<ImplementationRelation>,

    /// Type reference relationships (entity → type it uses in annotations)
    pub type_references: Vec<TypeReference>,
}

impl CodeIR {
    /// Create a new empty IR
    pub fn new(file_path: PathBuf) -> Self {
        Self {
            file_path,
            ..Default::default()
        }
    }

    /// Total number of entities
    pub fn entity_count(&self) -> usize {
        self.functions.len()
            + self.classes.len()
            + self.traits.len()
            + if self.module.is_some() { 1 } else { 0 }
    }

    /// Total number of relationships
    pub fn relationship_count(&self) -> usize {
        self.calls.len()
            + self.imports.len()
            + self.inheritance.len()
            + self.implementations.len()
            + self.type_references.len()
    }

    /// Add a module entity
    pub fn set_module(&mut self, module: ModuleEntity) {
        self.module = Some(module);
    }

    /// Add a function
    pub fn add_function(&mut self, func: FunctionEntity) {
        self.functions.push(func);
    }

    /// Add a class
    pub fn add_class(&mut self, class: ClassEntity) {
        self.classes.push(class);
    }

    /// Add a trait
    pub fn add_trait(&mut self, trait_entity: TraitEntity) {
        self.traits.push(trait_entity);
    }

    /// Add a call relationship
    pub fn add_call(&mut self, call: CallRelation) {
        self.calls.push(call);
    }

    /// Add an import relationship
    pub fn add_import(&mut self, import: ImportRelation) {
        self.imports.push(import);
    }

    /// Add an inheritance relationship
    pub fn add_inheritance(&mut self, inheritance: InheritanceRelation) {
        self.inheritance.push(inheritance);
    }

    /// Add an implementation relationship
    pub fn add_implementation(&mut self, implementation: ImplementationRelation) {
        self.implementations.push(implementation);
    }

    /// Add a type reference relationship
    pub fn add_type_reference(&mut self, type_ref: TypeReference) {
        self.type_references.push(type_ref);
    }
}
