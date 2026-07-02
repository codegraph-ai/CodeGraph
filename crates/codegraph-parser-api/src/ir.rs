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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn type_references_count_toward_relationships() {
        let mut ir = CodeIR::new(PathBuf::from("t.rs"));
        assert_eq!(ir.relationship_count(), 0);
        ir.add_type_reference(TypeReference::new("f", "Widget", 3));
        assert_eq!(ir.type_references.len(), 1);
        assert_eq!(ir.relationship_count(), 1);
        assert_eq!(ir.type_references[0].type_name, "Widget");
    }

    #[test]
    fn default_is_empty() {
        let ir = CodeIR::default();
        assert_eq!(ir.file_path, PathBuf::new());
        assert_eq!(ir.entity_count(), 0);
        assert_eq!(ir.relationship_count(), 0);
    }

    #[test]
    fn new_sets_path_and_leaves_rest_empty() {
        let ir = CodeIR::new(PathBuf::from("src/lib.rs"));
        assert_eq!(ir.file_path, PathBuf::from("src/lib.rs"));
        assert!(ir.module.is_none());
        assert!(ir.functions.is_empty());
        assert!(ir.classes.is_empty());
        assert!(ir.traits.is_empty());
        assert!(ir.calls.is_empty());
        assert!(ir.imports.is_empty());
        assert!(ir.inheritance.is_empty());
        assert!(ir.implementations.is_empty());
        assert!(ir.type_references.is_empty());
        assert_eq!(ir.entity_count(), 0);
        assert_eq!(ir.relationship_count(), 0);
    }

    #[test]
    fn set_module_counts_as_one_entity() {
        let mut ir = CodeIR::new(PathBuf::from("m.rs"));
        assert_eq!(ir.entity_count(), 0);
        ir.set_module(ModuleEntity::new("m", "m.rs", "rust"));
        assert!(ir.module.is_some());
        assert_eq!(ir.entity_count(), 1);
        assert_eq!(ir.relationship_count(), 0);
    }

    #[test]
    fn add_entities_increment_entity_count() {
        let mut ir = CodeIR::new(PathBuf::from("e.rs"));
        ir.set_module(ModuleEntity::new("e", "e.rs", "rust"));
        ir.add_function(FunctionEntity::new("f", 1, 2));
        ir.add_class(ClassEntity::new("C", 3, 4));
        ir.add_trait(TraitEntity::new("T", 5, 6));
        assert_eq!(ir.functions.len(), 1);
        assert_eq!(ir.classes.len(), 1);
        assert_eq!(ir.traits.len(), 1);
        // module + function + class + trait
        assert_eq!(ir.entity_count(), 4);
        assert_eq!(ir.relationship_count(), 0);
    }

    #[test]
    fn add_relationships_increment_relationship_count() {
        let mut ir = CodeIR::new(PathBuf::from("r.rs"));
        ir.add_call(CallRelation::new("a", "b", 1));
        ir.add_import(ImportRelation::new("a", "std::io"));
        ir.add_inheritance(InheritanceRelation::new("Child", "Parent"));
        ir.add_implementation(ImplementationRelation::new("S", "Trait"));
        ir.add_type_reference(TypeReference::new("f", "Widget", 9));
        assert_eq!(ir.calls.len(), 1);
        assert_eq!(ir.imports.len(), 1);
        assert_eq!(ir.inheritance.len(), 1);
        assert_eq!(ir.implementations.len(), 1);
        assert_eq!(ir.type_references.len(), 1);
        assert_eq!(ir.relationship_count(), 5);
        assert_eq!(ir.entity_count(), 0);
    }

    #[test]
    fn add_methods_append_in_order() {
        let mut ir = CodeIR::new(PathBuf::from("o.rs"));
        ir.add_function(FunctionEntity::new("first", 1, 1));
        ir.add_function(FunctionEntity::new("second", 2, 2));
        assert_eq!(ir.functions[0].name, "first");
        assert_eq!(ir.functions[1].name, "second");
    }

    #[test]
    fn clone_is_an_independent_deep_copy() {
        let mut original = CodeIR::new(PathBuf::from("c.rs"));
        original.set_module(ModuleEntity::new("c", "c.rs", "rust"));
        original.add_function(FunctionEntity::new("f", 1, 2));
        original.add_call(CallRelation::new("a", "b", 1));

        let mut cloned = original.clone();
        // The clone starts out equal to the original.
        assert_eq!(cloned.file_path, PathBuf::from("c.rs"));
        assert_eq!(cloned.entity_count(), 2);
        assert_eq!(cloned.relationship_count(), 1);

        // Mutating the clone must not affect the original (deep, not shared).
        cloned.add_function(FunctionEntity::new("g", 3, 4));
        cloned.add_call(CallRelation::new("c", "d", 2));
        assert_eq!(cloned.entity_count(), 3);
        assert_eq!(cloned.relationship_count(), 2);
        assert_eq!(original.entity_count(), 2);
        assert_eq!(original.relationship_count(), 1);
        assert_eq!(original.functions.len(), 1);
        assert_eq!(original.calls.len(), 1);
    }

    #[test]
    fn clone_preserves_module_presence() {
        // A module-less IR clones to a module-less IR (the None arm of the Option).
        let without = CodeIR::new(PathBuf::from("n.rs"));
        assert!(without.clone().module.is_none());

        // A populated module survives the clone with its fields intact.
        let mut with = CodeIR::new(PathBuf::from("y.rs"));
        with.set_module(ModuleEntity::new("mod_y", "y.rs", "rust"));
        let cloned = with.clone();
        assert_eq!(cloned.entity_count(), 1);
        let module = cloned
            .module
            .as_ref()
            .expect("cloned module should be present");
        assert_eq!(module.name, "mod_y");
    }
}
