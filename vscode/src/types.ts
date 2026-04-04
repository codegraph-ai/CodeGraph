// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import { Position, Range, Location } from 'vscode-languageclient';

// ==========================================
// Dependency Graph Types
// ==========================================

export interface DependencyGraphParams {
    uri: string;
    depth?: number;
    includeExternal?: boolean;
    direction?: 'imports' | 'importedBy' | 'both';
}

export interface DependencyNode {
    id: string;
    label: string;
    type: 'module' | 'package' | 'file';
    language: string;
    uri: string;
    metadata?: Record<string, unknown>;
}

export interface DependencyEdge {
    from: string;
    to: string;
    type: 'import' | 'require' | 'use';
    metadata?: Record<string, unknown>;
}

export interface DependencyGraphResponse {
    nodes: DependencyNode[];
    edges: DependencyEdge[];
}

// ==========================================
// Call Graph Types
// ==========================================

export interface CallGraphParams {
    uri: string;
    position: Position;
    direction?: 'callers' | 'callees' | 'both';
    depth?: number;
    includeExternal?: boolean;
}

export interface FunctionNode {
    id: string;
    name: string;
    signature: string;
    uri: string;
    range: Range;
    language: string;
    metrics?: {
        complexity?: number;
        linesOfCode?: number;
        callCount?: number;
    };
}

export interface CallEdge {
    from: string;
    to: string;
    callSites: Location[];
    isRecursive?: boolean;
}

export interface CallGraphResponse {
    root: FunctionNode;
    nodes: FunctionNode[];
    edges: CallEdge[];
}

// ==========================================
// AI Context Types
// ==========================================

export interface AIContextParams {
    uri: string;
    position: Position;
    contextType: 'explain' | 'modify' | 'debug' | 'test';
    maxTokens?: number;
}

export interface PrimaryContext {
    type: 'function' | 'class' | 'module';
    name: string;
    code: string;
    language: string;
    location: Location;
}

export interface RelatedSymbol {
    name: string;
    relationship: 'calls' | 'called_by' | 'uses' | 'used_by' | 'inherits' | 'implements' | 'tests' | 'similar';
    code: string;
    location: Location;
    relevanceScore: number;
}

export interface DependencyInfo {
    name: string;
    type: 'import' | 'type_dependency';
    code?: string;
}

export interface UsageExample {
    code: string;
    location: Location;
    description?: string;
}

export interface ArchitectureInfo {
    module: string;
    layer?: string;
    neighbors: string[];
}

export interface AIContextResponse {
    primaryContext: PrimaryContext;
    relatedSymbols: RelatedSymbol[];
    dependencies: DependencyInfo[];
    usageExamples?: UsageExample[];
    architecture?: ArchitectureInfo;
    metadata: {
        totalTokens: number;
        queryTime: number;
    };
}

export interface GitHistorySearchResponse {
    query: string;
    results: Array<{
        hash: string;
        fullHash?: string;
        subject: string;
        content?: string;
        kind?: string;
        score?: number;
        author?: string;
        date?: string;
        files?: string[];
        source: 'semantic' | 'keyword' | 'time_range';
    }>;
    metadata: { total: number; queryTime: number; semanticMatches?: number; keywordMatches?: number };
}

export interface CuratedContextResponse {
    query: string;
    symbols: Array<{ name: string; kind: string; file: string; line: number; score: number; matchReason?: string; code?: string }>;
    dependencies: Array<{ name: string; kind: string; file: string; relationship: string; code?: string }>;
    memories: Array<{ title: string; content: string; kind: string; relatedFile: string }>;
    metadata: {
        totalTokens: number;
        maxTokens: number;
        queryTime: number;
        symbolsFound: number;
        symbolsIncluded: number;
        dependenciesIncluded: number;
        memoriesIncluded: number;
    };
}

export interface EditContextResponse {
    symbol: {
        name: string;
        type: string;
        code: string;
        language: string;
        location: { uri: string; range: { start: Position; end: Position } };
    };
    callers: Array<{ name: string; code?: string; file: string; line: number }>;
    tests: Array<{ name: string; relationship: string; code?: string }>;
    memories: Array<{ id: string; title: string; content: string; kind: string }>;
    recentChanges: Array<{ hash: string; subject: string; author: string; date: string }>;
    metadata: {
        totalTokens: number;
        maxTokens: number;
        queryTime: number;
    };
}

// ==========================================
// Impact Analysis Types
// ==========================================

export interface ImpactAnalysisParams {
    uri: string;
    position: Position;
    analysisType: 'modify' | 'delete' | 'rename';
}

export interface DirectImpact {
    uri: string;
    range: Range;
    type: 'caller' | 'reference' | 'subclass' | 'implementation';
    severity: 'breaking' | 'warning' | 'info';
}

export interface IndirectImpact {
    uri: string;
    path: string[];
    severity: 'breaking' | 'warning' | 'info';
}

export interface AffectedTest {
    uri: string;
    testName: string;
}

export interface ImpactAnalysisResponse {
    directImpact: DirectImpact[];
    indirectImpact: IndirectImpact[];
    affectedTests: AffectedTest[];
    summary: {
        filesAffected: number;
        breakingChanges: number;
        warnings: number;
    };
}

// ==========================================
// Related Tests Types
// ==========================================

export interface RelatedTestsParams {
    uri: string;
    position: Position;
    limit?: number;
}

export interface RelatedTest {
    uri: string;
    testName: string;
    relationship: string;
    range: Range;
}

export interface RelatedTestsResponse {
    tests: RelatedTest[];
    truncated?: boolean;
}

// ==========================================
// Parser Metrics Types
// ==========================================

export interface ParserMetricsParams {
    language?: string;
}

export interface ParserMetric {
    language: string;
    filesAttempted: number;
    filesSucceeded: number;
    filesFailed: number;
    totalEntities: number;
    totalRelationships: number;
    totalParseTimeMs: number;
    avgParseTimeMs: number;
}

export interface ParserMetricsResponse {
    metrics: ParserMetric[];
    totals: {
        filesAttempted: number;
        filesSucceeded: number;
        filesFailed: number;
        totalEntities: number;
        successRate: number;
    };
}

// ==========================================
// Graph Visualization Types (for webview)
// ==========================================

export interface GraphNode {
    id: string;
    label: string;
    type: string;
    language?: string;
    x?: number;
    y?: number;
}

export interface GraphEdge {
    from: string;
    to: string;
    type: string;
}

export interface GraphData {
    nodes: GraphNode[];
    edges: GraphEdge[];
}

// ==========================================
// Code Metrics Types
// ==========================================

export interface ComplexityParams {
    uri: string;
    line?: number;
    threshold?: number;
    includeMetrics?: boolean;
}

export interface ComplexityDetails {
    complexityBranches: number;
    complexityLoops: number;
    complexityLogicalOps: number;
    complexityNesting: number;
    complexityExceptions: number;
    complexityEarlyReturns: number;
    linesOfCode: number;
}

export interface LocationInfo {
    uri: string;
    range: Range;
}

export interface FunctionComplexity {
    name: string;
    complexity: number;
    grade: string;
    location: LocationInfo;
    details: ComplexityDetails;
}

export interface FileSummary {
    totalFunctions: number;
    averageComplexity: number;
    maxComplexity: number;
    functionsAboveThreshold: number;
    overallGrade: string;
}

export interface ComplexityResponse {
    functions: FunctionComplexity[];
    fileSummary: FileSummary;
    recommendations: string[];
}

// ==========================================
// Unused Code Detection Types
// ==========================================

export interface UnusedCodeParams {
    uri?: string;
    scope: 'file' | 'module' | 'workspace';
    includeTests?: boolean;
    confidence?: number;
}

export interface UnusedItem {
    itemType: string;
    name: string;
    location: LocationInfo;
    confidence: number;
    reason: string;
    safeToRemove: boolean;
}

export interface UnusedByType {
    functions: number;
    classes: number;
    imports: number;
    variables: number;
}

export interface UnusedSummary {
    totalItems: number;
    byType: UnusedByType;
    safeDeletions: number;
    estimatedLinesRemovable: number;
}

export interface UnusedCodeResponse {
    unusedItems: UnusedItem[];
    summary: UnusedSummary;
}

// ==========================================
// Coupling Analysis Types
// ==========================================

export interface CouplingParams {
    uri: string;
    includeExternal?: boolean;
    depth?: number;
}

export interface CouplingMetrics {
    afferent: number;
    efferent: number;
    instability: number;
    dependents: string[];
    dependencies: string[];
}

export interface CohesionMetrics {
    score: number;
    cohesionType: string;
    internalReferenceRatio: number;
}

export interface ArchViolation {
    violationType: string;
    severity: string;
    description: string;
    suggestion: string;
}

export interface CouplingResponse {
    coupling: CouplingMetrics;
    cohesion: CohesionMetrics;
    violations: ArchViolation[];
    recommendations: string[];
}

// ==========================================
// AI Agent Query Primitives Types
// ==========================================

export interface SymbolSearchParams {
    query: string;
    scope?: 'workspace' | 'module' | 'file';
    symbolTypes?: ('function' | 'class' | 'variable' | 'module' | 'interface' | 'type')[];
    limit?: number;
    includePrivate?: boolean;
}

export interface SymbolLocation {
    file: string;
    line: number;
    column: number;
    endLine: number;
    endColumn: number;
}

export interface SymbolInfo {
    name: string;
    kind: string;
    location: SymbolLocation;
    signature?: string;
    docstring?: string;
    isPublic: boolean;
}

export interface SymbolMatch {
    nodeId: string;
    symbol: SymbolInfo;
    score: number;
    matchReason: string;
}

export interface SymbolSearchResponse {
    results: SymbolMatch[];
    totalMatches: number;
    queryTimeMs: number;
}

export interface FindByImportsParams {
    libraries: string[];
    matchMode?: 'exact' | 'prefix' | 'fuzzy';
}

export interface FindByImportsResponse {
    results: SymbolMatch[];
    queryTimeMs: number;
}

export interface FindEntryPointsParams {
    entryType?: 'http_handler' | 'cli_command' | 'public_api' | 'event_handler' | 'test_entry' | 'main';
}

export interface EntryPoint {
    nodeId: string;
    entryType: string;
    route?: string;
    method?: string;
    description?: string;
    symbol: SymbolInfo;
}

export interface FindEntryPointsResponse {
    entryPoints: EntryPoint[];
    totalFound: number;
}

export interface TraverseGraphParams {
    startNodeId?: string;
    uri?: string;
    line?: number;
    direction?: 'outgoing' | 'incoming' | 'both';
    depth?: number;
    filterSymbolTypes?: ('function' | 'class' | 'variable' | 'module' | 'interface' | 'type')[];
    maxNodes?: number;
}

export interface TraversalNode {
    nodeId: string;
    depth: number;
    path: string[];
    edgeType: string;
    symbol: SymbolInfo;
}

export interface TraverseGraphResponse {
    nodes: TraversalNode[];
    queryTimeMs: number;
}

export interface GetCallersParams {
    nodeId?: string;
    uri?: string;
    line?: number;
    depth?: number;
}

export interface CallInfo {
    nodeId: string;
    symbol: SymbolInfo;
    callSite: SymbolLocation;
    depth: number;
}

export interface GetCallersResponse {
    callers: CallInfo[];
    queryTimeMs: number;
}

export interface GetDetailedInfoParams {
    nodeId?: string;
    uri?: string;
    line?: number;
    includeCallers?: boolean;
    includeCallees?: boolean;
}

export interface DetailedSymbolResponse {
    symbol: SymbolInfo;
    callers: CallInfo[];
    callees: CallInfo[];
    complexity?: number;
    linesOfCode: number;
    isPublic: boolean;
    isDeprecated: boolean;
    referenceCount: number;
}

export interface FindBySignatureParams {
    namePattern?: string;
    returnType?: string;
    paramCount?: {
        min: number;
        max: number;
    };
    modifiers?: ('public' | 'private' | 'static' | 'async' | 'const')[];
}

export interface FindBySignatureResponse {
    results: SymbolMatch[];
    queryTimeMs: number;
}

// ==========================================
// Memory Layer Types
// ==========================================

export type MemoryKind = 'debug_context' | 'architectural_decision' | 'known_issue' | 'convention' | 'project_context';

export interface MemoryCodeLink {
    nodeId: string;
    nodeType: string;
}

export interface MemoryStoreParams {
    kind: MemoryKind;
    title: string;
    content: string;
    tags?: string[];
    codeLinks?: MemoryCodeLink[];
    confidence?: number;
    // Kind-specific fields
    problem?: string;           // debug_context
    solution?: string;          // debug_context
    decision?: string;          // architectural_decision
    rationale?: string;         // architectural_decision
    description?: string;       // known_issue, convention, project_context
    severity?: 'critical' | 'high' | 'medium' | 'low';  // known_issue
    name?: string;              // convention
    topic?: string;             // project_context
}

export interface MemoryStoreResponse {
    id: string;
    success: boolean;
}

export interface MemorySearchParams {
    query: string;
    limit?: number;
    tags?: string[];
    kinds?: MemoryKind[];
    currentOnly?: boolean;
    codeContext?: string[];
}

export interface MemorySearchResult {
    id: string;
    kind: string;
    title: string;
    content: string;
    tags: string[];
    score: number;
    isCurrent: boolean;
}

export interface MemorySearchResponse {
    results: MemorySearchResult[];
    total: number;
}

export interface MemoryGetParams {
    id: string;
}

export interface MemoryGetResponse {
    id: string;
    kind: Record<string, unknown>;
    title: string;
    content: string;
    tags: string[];
    codeLinks: MemoryCodeLink[];
    confidence: number;
    isCurrent: boolean;
    createdAt: string;
    validFrom?: string;
}

export interface MemoryInvalidateParams {
    id: string;
}

export interface MemoryInvalidateResponse {
    success: boolean;
}

export interface MemoryListParams {
    kinds?: MemoryKind[];
    tags?: string[];
    currentOnly?: boolean;
    limit?: number;
    offset?: number;
}

export interface MemoryListResponse {
    memories: MemorySearchResult[];
    total: number;
    hasMore: boolean;
}

export interface MemoryContextParams {
    uri: string;
    position?: Position;
    limit?: number;
    kinds?: MemoryKind[];
}

export interface ContextMemory {
    id: string;
    kind: string;
    title: string;
    content: string;
    tags: string[];
    relevanceScore: number;
    relevanceReason: string;
}

export interface MemoryContextResponse {
    memories: ContextMemory[];
}

export interface MemoryStatsResponse {
    totalMemories: number;
    currentMemories: number;
    invalidatedMemories: number;
    byKind: Record<string, number>;
    byTag: Record<string, number>;
}

// ==========================================
// Git Mining Types
// ==========================================

export interface GitMiningResponse {
    file?: string;
    commitsProcessed: number;
    memoriesCreated: number;
    commitsSkipped: number;
    memoryIds: string[];
    warnings: string[];
    hotspotsDetected?: number;
    couplingsDetected?: number;
}
