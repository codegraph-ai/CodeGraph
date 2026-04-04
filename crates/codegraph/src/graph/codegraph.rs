// Copyright 2024-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

//! Main CodeGraph interface for graph operations.

use super::property::PropertyMap;
use super::types::{Direction, Edge, EdgeId, EdgeType, Node, NodeId, NodeType};
use crate::error::{GraphError, Result};
use crate::storage::StorageBackend;
use log::{debug, info, trace};
use std::collections::{HashMap, HashSet};
#[cfg(feature = "rocksdb-backend")]
use std::path::Path;

/// The main code graph database.
///
/// `CodeGraph` provides the primary interface for storing and querying code relationships.
/// All operations are explicit with no hidden behavior.
pub struct CodeGraph {
    storage: Box<dyn StorageBackend>,
    // Node and edge counters for monotonic ID generation
    node_counter: NodeId,
    edge_counter: EdgeId,
    // In-memory caches for fast lookups
    nodes: HashMap<NodeId, Node>,
    edges: HashMap<EdgeId, Edge>,
    // Adjacency indexes for O(1) neighbor lookups
    adjacency_out: HashMap<NodeId, HashSet<EdgeId>>,
    adjacency_in: HashMap<NodeId, HashSet<EdgeId>>,
}

impl CodeGraph {
    /// Open or create a code graph with the given storage backend.
    ///
    /// This is the explicit way to create a graph. No automatic scanning or parsing.
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::Storage`] if the backend cannot be initialized.
    pub fn with_backend(backend: Box<dyn StorageBackend>) -> Result<Self> {
        let mut graph = Self {
            storage: backend,
            node_counter: 0,
            edge_counter: 0,
            nodes: HashMap::new(),
            edges: HashMap::new(),
            adjacency_out: HashMap::new(),
            adjacency_in: HashMap::new(),
        };

        // Load graph state from storage
        graph.rebuild_from_storage()?;

        Ok(graph)
    }

    /// Open a persistent code graph at the given path.
    ///
    /// Uses RocksDB for production-grade persistent storage.
    ///
    /// # Example
    ///
    /// ```no_run
    /// use codegraph::CodeGraph;
    /// use std::path::Path;
    ///
    /// let graph = CodeGraph::open(Path::new("./my_project.graph")).unwrap();
    /// ```
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::Storage`] if the database cannot be opened.
    #[cfg(feature = "rocksdb-backend")]
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        use crate::storage::RocksDBBackend;
        info!("Opening graph at path: {:?}", path.as_ref());
        let backend = RocksDBBackend::open(path)?;
        Self::with_backend(Box::new(backend))
    }

    /// Create an in-memory code graph for testing.
    ///
    /// **Warning**: All data is lost when the graph is dropped.
    /// Only use for testing.
    pub fn in_memory() -> Result<Self> {
        use crate::storage::MemoryBackend;
        let backend = MemoryBackend::new();
        Self::with_backend(Box::new(backend))
    }

    /// Add a node to the graph.
    ///
    /// # Parameters
    ///
    /// - `node_type`: Type of code entity
    /// - `properties`: Metadata for the node
    ///
    /// # Returns
    ///
    /// The unique ID assigned to the created node.
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::Storage`] if persistence fails.
    pub fn add_node(&mut self, node_type: NodeType, properties: PropertyMap) -> Result<NodeId> {
        let node_id = self.next_node_id();
        debug!("Adding node: id={node_id}, type={node_type}");
        let node = Node::new(node_id, node_type, properties);

        // Serialize and store
        let key = format!("node:{node_id}");
        let value = serde_json::to_vec(&node)
            .map_err(|e| GraphError::serialization("Failed to serialize node", Some(e)))?;

        self.storage.put(key.as_bytes(), &value)?;

        // Update in-memory cache
        self.nodes.insert(node_id, node);
        trace!("Node {node_id} added successfully");

        Ok(node_id)
    }

    /// Get a node by ID (immutable).
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::NodeNotFound`] if the node doesn't exist.
    pub fn get_node(&self, id: NodeId) -> Result<&Node> {
        self.nodes.get(&id).ok_or_else(|| GraphError::NodeNotFound {
            node_id: id.to_string(),
        })
    }

    /// Get a mutable reference to a node by ID.
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::NodeNotFound`] if the node doesn't exist.
    pub fn get_node_mut(&mut self, id: NodeId) -> Result<&mut Node> {
        self.nodes
            .get_mut(&id)
            .ok_or_else(|| GraphError::NodeNotFound {
                node_id: id.to_string(),
            })
    }

    /// Update properties of an existing node.
    ///
    /// Merges new properties with existing ones (overwrites duplicates).
    ///
    /// # Errors
    ///
    /// Returns error if node not found or persistence fails.
    pub fn update_node_properties(&mut self, id: NodeId, properties: PropertyMap) -> Result<()> {
        let node = self.get_node_mut(id)?;

        // Merge properties
        for (key, value) in properties.iter() {
            node.properties.insert(key.clone(), value.clone());
        }

        // Persist updated node
        let key = format!("node:{id}");
        let value = serde_json::to_vec(node)
            .map_err(|e| GraphError::serialization("Failed to serialize node", Some(e)))?;

        self.storage.put(key.as_bytes(), &value)?;

        Ok(())
    }

    /// Delete a node and all its connected edges.
    ///
    /// # Errors
    ///
    /// Returns error if node not found or deletion fails.
    pub fn delete_node(&mut self, id: NodeId) -> Result<()> {
        debug!("Deleting node: id={id}");
        // Verify node exists
        if !self.nodes.contains_key(&id) {
            return Err(GraphError::NodeNotFound {
                node_id: id.to_string(),
            });
        }

        // Find and delete all connected edges
        let mut edges_to_delete = Vec::new();

        if let Some(out_edges) = self.adjacency_out.get(&id) {
            edges_to_delete.extend(out_edges.iter().copied());
        }

        if let Some(in_edges) = self.adjacency_in.get(&id) {
            edges_to_delete.extend(in_edges.iter().copied());
        }

        trace!(
            "Deleting {} connected edges for node {}",
            edges_to_delete.len(),
            id
        );
        for edge_id in edges_to_delete {
            self.delete_edge(edge_id)?;
        }

        // Delete node from storage
        let key = format!("node:{id}");
        self.storage.delete(key.as_bytes())?;

        // Remove from cache
        self.nodes.remove(&id);

        Ok(())
    }

    /// Add an edge to the graph.
    ///
    /// # Parameters
    ///
    /// - `source_id`: Source node ID
    /// - `target_id`: Target node ID
    /// - `edge_type`: Type of relationship
    /// - `properties`: Metadata for the edge (e.g., line number)
    ///
    /// # Returns
    ///
    /// The unique ID assigned to the created edge.
    ///
    /// # Errors
    ///
    /// Returns error if source or target node doesn't exist or storage fails.
    pub fn add_edge(
        &mut self,
        source_id: NodeId,
        target_id: NodeId,
        edge_type: EdgeType,
        properties: PropertyMap,
    ) -> Result<EdgeId> {
        debug!("Adding edge: source={source_id}, target={target_id}, type={edge_type}");
        // Verify nodes exist
        self.get_node(source_id)?;
        self.get_node(target_id)?;

        let edge_id = self.next_edge_id();
        let edge = Edge::new(edge_id, source_id, target_id, edge_type, properties);

        // Serialize and store
        let key = format!("edge:{edge_id}");
        let value = serde_json::to_vec(&edge)
            .map_err(|e| GraphError::serialization("Failed to serialize edge", Some(e)))?;

        self.storage.put(key.as_bytes(), &value)?;

        // Update adjacency indexes
        self.adjacency_out
            .entry(source_id)
            .or_default()
            .insert(edge_id);

        self.adjacency_in
            .entry(target_id)
            .or_default()
            .insert(edge_id);

        // Update in-memory cache
        self.edges.insert(edge_id, edge);

        Ok(edge_id)
    }

    /// Get an edge by ID.
    ///
    /// # Errors
    ///
    /// Returns [`GraphError::EdgeNotFound`] if the edge doesn't exist.
    pub fn get_edge(&self, id: EdgeId) -> Result<&Edge> {
        self.edges.get(&id).ok_or_else(|| GraphError::EdgeNotFound {
            edge_id: id.to_string(),
        })
    }

    /// Delete an edge.
    ///
    /// # Errors
    ///
    /// Returns error if edge not found or deletion fails.
    pub fn delete_edge(&mut self, id: EdgeId) -> Result<()> {
        debug!("Deleting edge: id={id}");
        let edge = self
            .edges
            .get(&id)
            .ok_or_else(|| GraphError::EdgeNotFound {
                edge_id: id.to_string(),
            })?;

        let source_id = edge.source_id;
        let target_id = edge.target_id;

        // Delete from storage
        let key = format!("edge:{id}");
        self.storage.delete(key.as_bytes())?;

        // Update adjacency indexes
        if let Some(out_edges) = self.adjacency_out.get_mut(&source_id) {
            out_edges.remove(&id);
        }

        if let Some(in_edges) = self.adjacency_in.get_mut(&target_id) {
            in_edges.remove(&id);
        }

        // Remove from cache
        self.edges.remove(&id);

        Ok(())
    }

    /// Get all neighbor nodes connected by edges in the specified direction.
    ///
    /// # Parameters
    ///
    /// - `node_id`: The node to find neighbors for
    /// - `direction`: Which edges to follow (Outgoing, Incoming, or Both)
    ///
    /// # Returns
    ///
    /// Vector of neighbor node IDs.
    ///
    /// # Errors
    ///
    /// Returns error if node not found.
    pub fn get_neighbors(&self, node_id: NodeId, direction: Direction) -> Result<Vec<NodeId>> {
        // Verify node exists
        self.get_node(node_id)?;

        let mut neighbors = HashSet::new();

        match direction {
            Direction::Outgoing => {
                if let Some(out_edges) = self.adjacency_out.get(&node_id) {
                    for edge_id in out_edges {
                        if let Ok(edge) = self.get_edge(*edge_id) {
                            neighbors.insert(edge.target_id);
                        }
                    }
                }
            }
            Direction::Incoming => {
                if let Some(in_edges) = self.adjacency_in.get(&node_id) {
                    for edge_id in in_edges {
                        if let Ok(edge) = self.get_edge(*edge_id) {
                            neighbors.insert(edge.source_id);
                        }
                    }
                }
            }
            Direction::Both => {
                if let Some(out_edges) = self.adjacency_out.get(&node_id) {
                    for edge_id in out_edges {
                        if let Ok(edge) = self.get_edge(*edge_id) {
                            neighbors.insert(edge.target_id);
                        }
                    }
                }
                if let Some(in_edges) = self.adjacency_in.get(&node_id) {
                    for edge_id in in_edges {
                        if let Ok(edge) = self.get_edge(*edge_id) {
                            neighbors.insert(edge.source_id);
                        }
                    }
                }
            }
        }

        Ok(neighbors.into_iter().collect())
    }

    /// Get all edges between two nodes.
    ///
    /// Returns all edges from source to target.
    pub fn get_edges_between(&self, source_id: NodeId, target_id: NodeId) -> Result<Vec<EdgeId>> {
        self.get_node(source_id)?;
        self.get_node(target_id)?;

        let mut edges = Vec::new();

        if let Some(out_edges) = self.adjacency_out.get(&source_id) {
            for edge_id in out_edges {
                if let Ok(edge) = self.get_edge(*edge_id) {
                    if edge.target_id == target_id {
                        edges.push(*edge_id);
                    }
                }
            }
        }

        Ok(edges)
    }

    /// Add multiple nodes in an atomic batch operation.
    ///
    /// Either all nodes are added or none are.
    ///
    /// # Returns
    ///
    /// Vector of assigned node IDs in the same order as input.
    pub fn add_nodes_batch(&mut self, nodes: Vec<(NodeType, PropertyMap)>) -> Result<Vec<NodeId>> {
        debug!("Adding batch of {} nodes", nodes.len());
        let mut node_ids = Vec::with_capacity(nodes.len());
        let mut operations = Vec::with_capacity(nodes.len());

        for (node_type, properties) in nodes {
            let node_id = self.next_node_id();
            let node = Node::new(node_id, node_type, properties);

            let key = format!("node:{node_id}");
            let value = serde_json::to_vec(&node)
                .map_err(|e| GraphError::serialization("Failed to serialize node", Some(e)))?;

            operations.push(crate::storage::BatchOperation::Put {
                key: key.into_bytes(),
                value,
            });

            self.nodes.insert(node_id, node);
            node_ids.push(node_id);
        }

        self.storage.write_batch(operations)?;
        trace!("Batch of {} nodes added successfully", node_ids.len());

        Ok(node_ids)
    }

    /// Add multiple edges in an atomic batch operation.
    ///
    /// Either all edges are added or none are.
    ///
    /// # Returns
    ///
    /// Vector of assigned edge IDs in the same order as input.
    pub fn add_edges_batch(
        &mut self,
        edges: Vec<(NodeId, NodeId, EdgeType, PropertyMap)>,
    ) -> Result<Vec<EdgeId>> {
        debug!("Adding batch of {} edges", edges.len());
        // Verify all nodes exist first
        for (source_id, target_id, _, _) in &edges {
            self.get_node(*source_id)?;
            self.get_node(*target_id)?;
        }

        let mut edge_ids = Vec::with_capacity(edges.len());
        let mut operations = Vec::with_capacity(edges.len());

        for (source_id, target_id, edge_type, properties) in edges {
            let edge_id = self.next_edge_id();
            let edge = Edge::new(edge_id, source_id, target_id, edge_type, properties);

            let key = format!("edge:{edge_id}");
            let value = serde_json::to_vec(&edge)
                .map_err(|e| GraphError::serialization("Failed to serialize edge", Some(e)))?;

            operations.push(crate::storage::BatchOperation::Put {
                key: key.into_bytes(),
                value,
            });

            // Update adjacency indexes
            self.adjacency_out
                .entry(source_id)
                .or_default()
                .insert(edge_id);

            self.adjacency_in
                .entry(target_id)
                .or_default()
                .insert(edge_id);

            self.edges.insert(edge_id, edge);
            edge_ids.push(edge_id);
        }

        self.storage.write_batch(operations)?;

        Ok(edge_ids)
    }

    /// Get the total number of nodes in the graph.
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Iterate over all (NodeId, Node) pairs in the graph.
    pub fn nodes_iter(&self) -> impl Iterator<Item = (&NodeId, &Node)> {
        self.nodes.iter()
    }

    /// Get the total number of edges in the graph.
    pub fn edge_count(&self) -> usize {
        self.edges.len()
    }

    /// Iterate over all nodes in the graph.
    ///
    /// Returns an iterator of `(NodeId, &Node)` pairs.
    pub fn iter_nodes(&self) -> impl Iterator<Item = (NodeId, &Node)> {
        self.nodes.iter().map(|(&id, node)| (id, node))
    }

    /// Iterate over all edges in the graph.
    ///
    /// Returns an iterator of `(EdgeId, &Edge)` pairs.
    pub fn iter_edges(&self) -> impl Iterator<Item = (EdgeId, &Edge)> {
        self.edges.iter().map(|(&id, edge)| (id, edge))
    }

    /// Clear all nodes and edges from the graph.
    ///
    /// This is a destructive operation that cannot be undone.
    pub fn clear(&mut self) -> Result<()> {
        // Delete all edges from storage first (avoids cascading delete issues)
        let edge_ids: Vec<_> = self.edges.keys().copied().collect();
        for edge_id in edge_ids {
            let key = format!("edge:{edge_id}");
            self.storage.delete(key.as_bytes())?;
        }

        // Delete all nodes from storage
        let node_ids: Vec<_> = self.nodes.keys().copied().collect();
        for node_id in node_ids {
            let key = format!("node:{node_id}");
            self.storage.delete(key.as_bytes())?;
        }

        // Clear in-memory caches
        self.edges.clear();
        self.nodes.clear();
        self.adjacency_out.clear();
        self.adjacency_in.clear();

        // Reset counters
        self.node_counter = 0;
        self.edge_counter = 0;

        // Persist counter reset
        self.save_counters()?;

        Ok(())
    }

    /// Create a new query builder for this graph.
    ///
    /// Returns a `QueryBuilder` that allows fluent chaining of filters
    /// to find specific nodes in the graph.
    ///
    /// # Examples
    ///
    /// ```
    /// use codegraph::{CodeGraph, NodeType};
    ///
    /// # fn example() -> codegraph::Result<()> {
    /// let mut graph = CodeGraph::in_memory()?;
    /// // ... populate graph ...
    ///
    /// // Find all public functions
    /// let results = graph.query()
    ///     .node_type(NodeType::Function)
    ///     .property("visibility", "public")
    ///     .execute()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn query<'a>(&'a self) -> crate::query::QueryBuilder<'a> {
        crate::query::QueryBuilder::new(self)
    }

    /// Explicitly flush any buffered writes to disk.
    ///
    /// Most operations are durable immediately, but this ensures WAL is synced.
    pub fn flush(&mut self) -> Result<()> {
        debug!("Flushing graph to disk");
        self.save_counters()?;
        self.storage.flush()?;
        trace!("Flush complete");
        Ok(())
    }

    /// Close the graph and ensure all data is persisted.
    pub fn close(mut self) -> Result<()> {
        self.flush()
    }

    /// Detach the persistent storage backend, switching to in-memory operation.
    ///
    /// Flushes all pending writes, then replaces the storage backend with a no-op
    /// [`MemoryBackend`]. This releases any database locks (e.g., RocksDB) while
    /// keeping all data accessible via in-memory caches.
    ///
    /// After detaching, new writes (add_node, add_edge) go to the in-memory backend
    /// and are NOT persisted. Use [`persist_to`](Self::persist_to) to write back to disk.
    pub fn detach_storage(&mut self) -> Result<()> {
        self.save_counters()?;
        self.storage.flush()?;
        self.storage = Box::new(crate::storage::MemoryBackend::new());
        info!("Storage detached — operating in memory-only mode");
        Ok(())
    }

    /// Persist all in-memory data to a storage backend.
    ///
    /// Opens the given backend, writes all nodes, edges, and counters,
    /// then drops the backend (releasing locks). The graph continues
    /// operating with its current (in-memory) backend.
    pub fn persist_to(&self, mut backend: Box<dyn StorageBackend>) -> Result<()> {
        info!(
            "Persisting {} nodes and {} edges to storage",
            self.nodes.len(),
            self.edges.len()
        );

        // Build batch of all nodes and edges
        let mut operations = Vec::with_capacity(self.nodes.len() + self.edges.len() + 1);

        for (&id, node) in &self.nodes {
            let key = format!("node:{id}");
            let value = serde_json::to_vec(node)
                .map_err(|e| GraphError::serialization("Failed to serialize node", Some(e)))?;
            operations.push(crate::storage::BatchOperation::Put {
                key: key.into_bytes(),
                value,
            });
        }

        for (&id, edge) in &self.edges {
            let key = format!("edge:{id}");
            let value = serde_json::to_vec(edge)
                .map_err(|e| GraphError::serialization("Failed to serialize edge", Some(e)))?;
            operations.push(crate::storage::BatchOperation::Put {
                key: key.into_bytes(),
                value,
            });
        }

        // Write counters
        let counters = serde_json::json!({
            "node_counter": self.node_counter,
            "edge_counter": self.edge_counter,
        });
        let counter_value = serde_json::to_vec(&counters)
            .map_err(|e| GraphError::serialization("Failed to serialize counters", Some(e)))?;
        operations.push(crate::storage::BatchOperation::Put {
            key: b"meta:counters".to_vec(),
            value: counter_value,
        });

        backend.write_batch(operations)?;
        backend.flush()?;

        info!("Persist complete");
        Ok(())
    }

    // Private helper methods

    fn next_node_id(&mut self) -> NodeId {
        let id = self.node_counter;
        self.node_counter += 1;
        id
    }

    fn next_edge_id(&mut self) -> EdgeId {
        let id = self.edge_counter;
        self.edge_counter += 1;
        id
    }

    fn save_counters(&mut self) -> Result<()> {
        let counters = serde_json::json!({
            "node_counter": self.node_counter,
            "edge_counter": self.edge_counter,
        });

        let value = serde_json::to_vec(&counters)
            .map_err(|e| GraphError::serialization("Failed to serialize counters", Some(e)))?;

        self.storage.put(b"meta:counters", &value)?;

        Ok(())
    }

    fn load_counters(&mut self) -> Result<()> {
        if let Some(value) = self.storage.get(b"meta:counters")? {
            let counters: serde_json::Value = serde_json::from_slice(&value).map_err(|e| {
                GraphError::serialization("Failed to deserialize counters", Some(e))
            })?;

            if let Some(node_counter) = counters.get("node_counter").and_then(|v| v.as_u64()) {
                self.node_counter = node_counter;
            }

            if let Some(edge_counter) = counters.get("edge_counter").and_then(|v| v.as_u64()) {
                self.edge_counter = edge_counter;
            }
        }

        Ok(())
    }

    fn rebuild_from_storage(&mut self) -> Result<()> {
        // Load counters
        self.load_counters()?;

        // Load all nodes
        let node_entries = self.storage.scan_prefix(b"node:")?;
        for (_, value) in node_entries {
            let node: Node = serde_json::from_slice(&value)
                .map_err(|e| GraphError::serialization("Failed to deserialize node", Some(e)))?;
            self.nodes.insert(node.id, node);
        }

        // Load all edges and rebuild indexes
        let edge_entries = self.storage.scan_prefix(b"edge:")?;
        for (_, value) in edge_entries {
            let edge: Edge = serde_json::from_slice(&value)
                .map_err(|e| GraphError::serialization("Failed to deserialize edge", Some(e)))?;

            self.adjacency_out
                .entry(edge.source_id)
                .or_default()
                .insert(edge.id);

            self.adjacency_in
                .entry(edge.target_id)
                .or_default()
                .insert(edge.id);

            self.edges.insert(edge.id, edge);
        }

        Ok(())
    }

    // ===== Algorithm Methods =====

    /// Perform Breadth-First Search traversal from a starting node.
    ///
    /// Returns all reachable nodes within the specified depth limit.
    ///
    /// # Parameters
    /// - `start`: Starting node ID
    /// - `direction`: Follow outgoing or incoming edges
    /// - `max_depth`: Optional maximum depth (None for unlimited)
    ///
    /// # Returns
    /// Vec of reachable node IDs (excluding the start node)
    pub fn bfs(
        &self,
        start: NodeId,
        direction: Direction,
        max_depth: Option<usize>,
    ) -> Result<Vec<NodeId>> {
        super::algorithms::bfs(self, start, direction, max_depth)
    }

    /// Perform Depth-First Search traversal from a starting node.
    ///
    /// Uses an iterative approach to avoid stack overflow.
    ///
    /// # Parameters
    /// - `start`: Starting node ID
    /// - `direction`: Follow outgoing or incoming edges
    /// - `max_depth`: Optional maximum depth (None for unlimited)
    ///
    /// # Returns
    /// Vec of reachable node IDs (excluding the start node)
    pub fn dfs(
        &self,
        start: NodeId,
        direction: Direction,
        max_depth: Option<usize>,
    ) -> Result<Vec<NodeId>> {
        super::algorithms::dfs(self, start, direction, max_depth)
    }

    /// Find all strongly connected components (SCCs) using Tarjan's algorithm.
    ///
    /// Returns groups of nodes that form circular dependencies.
    ///
    /// # Returns
    /// Vec of SCCs, where each SCC is a Vec of node IDs
    pub fn find_strongly_connected_components(&self) -> Result<Vec<Vec<NodeId>>> {
        super::algorithms::find_strongly_connected_components(self)
    }

    /// Find all paths between two nodes up to a maximum depth.
    ///
    /// # Parameters
    /// - `start`: Starting node ID
    /// - `end`: Target node ID
    /// - `max_depth`: Maximum path length (recommended: use Some value to prevent infinite loops)
    ///
    /// # Returns
    /// Vec of paths, where each path is a Vec of node IDs from start to end
    pub fn find_all_paths(
        &self,
        start: NodeId,
        end: NodeId,
        max_depth: Option<usize>,
    ) -> Result<Vec<Vec<NodeId>>> {
        super::algorithms::find_all_paths(self, start, end, max_depth)
    }

    // ===== Export Methods =====

    /// Export graph to Graphviz DOT format for visualization.
    ///
    /// **Warning**: Large graphs (>10K nodes) will produce warnings.
    /// Graphs over 100K nodes will fail.
    pub fn export_dot(&self) -> Result<String> {
        self.check_export_size()?;
        crate::export::export_dot(self)
    }

    /// Export graph to Graphviz DOT format with custom styling options.
    pub fn export_dot_styled(&self, options: crate::export::DotOptions) -> Result<String> {
        self.check_export_size()?;
        crate::export::export_dot_styled(self, options)
    }

    /// Export graph to D3.js-compatible JSON format.
    ///
    /// **Warning**: Large graphs (>10K nodes) will produce warnings.
    /// Graphs over 100K nodes will fail.
    pub fn export_json(&self) -> Result<String> {
        self.check_export_size()?;
        crate::export::export_json(self)
    }

    /// Export filtered subset of graph to JSON.
    pub fn export_json_filtered(
        &self,
        node_filter: impl Fn(&Node) -> bool,
        include_edges: bool,
    ) -> Result<String> {
        crate::export::export_json_filtered(self, node_filter, include_edges)
    }

    /// Export nodes to CSV file.
    ///
    /// **Warning**: Large graphs (>10K nodes) will produce warnings.
    /// Graphs over 100K nodes will fail.
    pub fn export_csv_nodes(&self, path: &std::path::Path) -> Result<()> {
        self.check_export_size()?;
        crate::export::export_csv_nodes(self, path)
    }

    /// Export edges to CSV file.
    ///
    /// **Warning**: Large graphs (>10K nodes) will produce warnings.
    /// Graphs over 100K nodes will fail.
    pub fn export_csv_edges(&self, path: &std::path::Path) -> Result<()> {
        self.check_export_size()?;
        crate::export::export_csv_edges(self, path)
    }

    /// Export both nodes and edges to separate CSV files (convenience method).
    pub fn export_csv(
        &self,
        nodes_path: &std::path::Path,
        edges_path: &std::path::Path,
    ) -> Result<()> {
        self.check_export_size()?;
        crate::export::export_csv(self, nodes_path, edges_path)
    }

    /// Export graph as RDF triples in N-Triples format.
    ///
    /// **Warning**: Large graphs (>10K nodes) will produce warnings.
    /// Graphs over 100K nodes will fail.
    pub fn export_triples(&self) -> Result<String> {
        self.check_export_size()?;
        crate::export::export_triples(self)
    }

    /// Check graph size for export operations and issue warnings/errors.
    fn check_export_size(&self) -> Result<()> {
        let node_count = self.node_count();

        if node_count > 100_000 {
            return Err(GraphError::InvalidOperation {
                message: format!(
                    "Graph too large for export ({node_count} nodes > 100K limit). Use filtering to export a subset."
                ),
            });
        }

        if node_count > 10_000 {
            eprintln!(
                "Warning: Exporting large graph ({node_count} nodes). Consider filtering for better performance."
            );
        }

        Ok(())
    }
}
