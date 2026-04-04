// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import * as vscode from 'vscode';
import { LanguageClient, RequestType } from 'vscode-languageclient/node';
import {
    DependencyGraphParams,
    DependencyGraphResponse,
    CallGraphParams,
    CallGraphResponse,
    GraphData,
} from '../types';

namespace GetDependencyGraphRequest {
    export const type = new RequestType<DependencyGraphParams, DependencyGraphResponse, void>(
        'codegraph/getDependencyGraph'
    );
}

namespace GetCallGraphRequest {
    export const type = new RequestType<CallGraphParams, CallGraphResponse, void>(
        'codegraph/getCallGraph'
    );
}

interface NodeLocation {
    uri: string;
    range: {
        start: { line: number; character: number };
        end: { line: number; character: number };
    };
}

// eslint-disable-next-line @typescript-eslint/no-unused-vars
namespace GetNodeLocationRequest {
    export const type = new RequestType<{ nodeId: string }, NodeLocation | null, void>(
        'codegraph/getNodeLocation'
    );
}

/**
 * Manages graph visualization webview panels.
 */
export class GraphVisualizationPanel {
    public static currentPanel: GraphVisualizationPanel | undefined;
    private readonly panel: vscode.WebviewPanel;
    private readonly extensionUri: vscode.Uri;
    private disposables: vscode.Disposable[] = [];

    private constructor(
        panel: vscode.WebviewPanel,
        extensionUri: vscode.Uri,
        private client: LanguageClient,
        graphType: 'dependency' | 'call',
        initialData: DependencyGraphResponse | CallGraphResponse
    ) {
        this.panel = panel;
        this.extensionUri = extensionUri;
        this.currentGraphType = graphType;
        this.expandedNodes = new Set();

        this.panel.webview.html = this.getWebviewContent();
        this.setupMessageHandlers();

        this.panel.onDidDispose(() => this.dispose(), null, this.disposables);

        // Send initial data
        this.panel.webview.postMessage({
            command: 'renderGraph',
            graphType,
            data: this.convertToGraphData(graphType, initialData),
        });
    }

    public static createOrShow(
        extensionUri: vscode.Uri,
        client: LanguageClient,
        graphType: 'dependency' | 'call',
        data: DependencyGraphResponse | CallGraphResponse
    ): GraphVisualizationPanel {
        const column = vscode.window.activeTextEditor
            ? vscode.window.activeTextEditor.viewColumn
            : undefined;

        if (GraphVisualizationPanel.currentPanel) {
            GraphVisualizationPanel.currentPanel.panel.reveal(column);
            // Update with new data
            GraphVisualizationPanel.currentPanel.panel.webview.postMessage({
                command: 'renderGraph',
                graphType,
                data: GraphVisualizationPanel.currentPanel.convertToGraphData(graphType, data),
            });
            return GraphVisualizationPanel.currentPanel;
        }

        const panel = vscode.window.createWebviewPanel(
            'codegraphVisualization',
            `CodeGraph ${graphType === 'dependency' ? 'Dependencies' : 'Call Graph'}`,
            column || vscode.ViewColumn.One,
            {
                enableScripts: true,
                retainContextWhenHidden: true,
                localResourceRoots: [
                    vscode.Uri.joinPath(extensionUri, 'webview', 'dist'),
                    vscode.Uri.joinPath(extensionUri, 'media'),
                ],
            }
        );

        GraphVisualizationPanel.currentPanel = new GraphVisualizationPanel(
            panel,
            extensionUri,
            client,
            graphType,
            data
        );

        return GraphVisualizationPanel.currentPanel;
    }

    private setupMessageHandlers(): void {
        this.panel.webview.onDidReceiveMessage(
            async (message) => {
                switch (message.command) {
                    case 'nodeClick':
                        await this.handleNodeClick(message.nodeId);
                        break;
                    case 'expandNode':
                        await this.expandNode(message.nodeId);
                        break;
                    case 'refresh':
                        await this.refresh(message.params);
                        break;
                    case 'exportSvg':
                        await this.exportSvg(message.svgContent);
                        break;
                    case 'exportJson':
                        await this.exportJson(message.data);
                        break;
                }
            },
            null,
            this.disposables
        );
    }

    private async exportSvg(svgContent: string): Promise<void> {
        const uri = await vscode.window.showSaveDialog({
            defaultUri: vscode.Uri.file('codegraph-export.svg'),
            filters: {
                'SVG Files': ['svg'],
            },
        });

        if (uri) {
            const encoder = new TextEncoder();
            await vscode.workspace.fs.writeFile(uri, encoder.encode(svgContent));
            vscode.window.showInformationMessage(`Graph exported to ${uri.fsPath}`);
        }
    }

    private async exportJson(data: GraphData): Promise<void> {
        const uri = await vscode.window.showSaveDialog({
            defaultUri: vscode.Uri.file('codegraph-export.json'),
            filters: {
                'JSON Files': ['json'],
            },
        });

        if (uri) {
            const encoder = new TextEncoder();
            const jsonContent = JSON.stringify(data, null, 2);
            await vscode.workspace.fs.writeFile(uri, encoder.encode(jsonContent));
            vscode.window.showInformationMessage(`Graph data exported to ${uri.fsPath}`);
        }
    }

    private async handleNodeClick(nodeId: string): Promise<void> {
        try {
            const location = await this.client.sendRequest('workspace/executeCommand', {
                command: 'codegraph.getNodeLocation',
                arguments: [{ nodeId }]
            }) as NodeLocation | null;

            if (location) {
                const uri = vscode.Uri.parse(location.uri);
                const range = new vscode.Range(
                    location.range.start.line,
                    location.range.start.character,
                    location.range.end.line,
                    location.range.end.character
                );

                const doc = await vscode.workspace.openTextDocument(uri);
                await vscode.window.showTextDocument(doc, {
                    selection: range,
                    preserveFocus: true,
                });
            }
        } catch (error) {
            console.error('Failed to navigate to node:', error);
        }
    }

    private currentGraphType: 'dependency' | 'call' = 'dependency';
    private currentUri: string = '';
    private expandedNodes: Set<string> = new Set();

    private async expandNode(nodeId: string): Promise<void> {
        // Prevent duplicate expansions
        if (this.expandedNodes.has(nodeId)) {
            return;
        }

        // Request more data centered on this node
        this.panel.webview.postMessage({
            command: 'expanding',
            nodeId,
        });

        try {
            // Get the location of the node to expand from
            const location = await this.client.sendRequest('workspace/executeCommand', {
                command: 'codegraph.getNodeLocation',
                arguments: [{ nodeId }]
            }) as NodeLocation | null;

            if (!location) {
                this.panel.webview.postMessage({
                    command: 'expandComplete',
                    nodeId,
                    success: false,
                    message: 'Could not find node location',
                });
                return;
            }

            let expandedData: GraphData;

            if (this.currentGraphType === 'dependency') {
                // Fetch dependency graph centered on this node's file
                const response = await this.client.sendRequest('workspace/executeCommand', {
                    command: 'codegraph.getDependencyGraph',
                    arguments: [{
                        uri: location.uri,
                        depth: 2,
                        includeExternal: false,
                        direction: 'both',
                    }]
                }) as DependencyGraphResponse;
                expandedData = this.convertToGraphData('dependency', response);
            } else {
                // Fetch call graph centered on this node's position
                const response = await this.client.sendRequest('workspace/executeCommand', {
                    command: 'codegraph.getCallGraph',
                    arguments: [{
                        uri: location.uri,
                        position: location.range.start,
                        depth: 2,
                        includeCallers: true,
                    }]
                }) as CallGraphResponse;
                expandedData = this.convertToGraphData('call', response);
            }

            // Mark node as expanded
            this.expandedNodes.add(nodeId);

            // Send expanded data to merge with current graph
            this.panel.webview.postMessage({
                command: 'expandComplete',
                nodeId,
                success: true,
                data: expandedData,
            });
        } catch (error) {
            console.error('Failed to expand node:', error);
            this.panel.webview.postMessage({
                command: 'expandComplete',
                nodeId,
                success: false,
                message: `Failed to expand: ${error}`,
            });
        }
    }

    private async refresh(params: DependencyGraphParams | CallGraphParams): Promise<void> {
        try {
            // Determine request type based on params
            if ('position' in params) {
                const response = await this.client.sendRequest(
                    GetCallGraphRequest.type,
                    params as CallGraphParams
                );
                this.panel.webview.postMessage({
                    command: 'renderGraph',
                    graphType: 'call',
                    data: this.convertToGraphData('call', response),
                });
            } else {
                const response = await this.client.sendRequest(
                    GetDependencyGraphRequest.type,
                    params as DependencyGraphParams
                );
                this.panel.webview.postMessage({
                    command: 'renderGraph',
                    graphType: 'dependency',
                    data: this.convertToGraphData('dependency', response),
                });
            }
        } catch (error) {
            this.panel.webview.postMessage({
                command: 'error',
                message: `Failed to refresh: ${error}`,
            });
        }
    }

    private convertToGraphData(
        graphType: 'dependency' | 'call',
        data: DependencyGraphResponse | CallGraphResponse
    ): GraphData {
        if (graphType === 'dependency') {
            const depData = data as DependencyGraphResponse;
            return {
                nodes: depData.nodes.map(n => ({
                    id: n.id,
                    label: n.label,
                    type: n.type,
                    language: n.language,
                })),
                edges: depData.edges.map(e => ({
                    from: e.from,
                    to: e.to,
                    type: e.type,
                })),
            };
        } else {
            const callData = data as CallGraphResponse;
            return {
                nodes: callData.nodes.map(n => ({
                    id: n.id,
                    label: n.name,
                    type: 'function',
                    language: n.language,
                })),
                edges: callData.edges.map(e => ({
                    from: e.from,
                    to: e.to,
                    type: e.isRecursive ? 'recursive' : 'call',
                })),
            };
        }
    }

    private getWebviewContent(): string {
        const nonce = getNonce();

        // Use inline styles and scripts for simplicity
        // In production, these would be loaded from separate files
        return `<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'nonce-${nonce}' 'unsafe-eval'; img-src data:;">
    <title>CodeGraph Visualization</title>
    <style>
        body {
            margin: 0;
            padding: 0;
            overflow: hidden;
            background: var(--vscode-editor-background, #1e1e1e);
            color: var(--vscode-foreground, #cccccc);
            font-family: var(--vscode-font-family, sans-serif);
        }
        #root {
            width: 100vw;
            height: 100vh;
        }
        #controls {
            position: absolute;
            top: 10px;
            left: 10px;
            z-index: 100;
            display: flex;
            gap: 8px;
        }
        button {
            background: var(--vscode-button-background, #0e639c);
            color: var(--vscode-button-foreground, #ffffff);
            border: none;
            padding: 6px 12px;
            cursor: pointer;
            border-radius: 2px;
        }
        button:hover {
            background: var(--vscode-button-hoverBackground, #1177bb);
        }
        #legend {
            position: absolute;
            bottom: 10px;
            left: 10px;
            z-index: 100;
            background: var(--vscode-editorWidget-background, #252526);
            padding: 10px;
            border-radius: 4px;
        }
        .legend-item {
            display: flex;
            align-items: center;
            margin: 4px 0;
        }
        .legend-color {
            width: 20px;
            height: 20px;
            border-radius: 50%;
            margin-right: 8px;
        }
        svg {
            width: 100%;
            height: 100%;
        }
        .node circle {
            stroke: var(--vscode-foreground, #fff);
            stroke-width: 2px;
            cursor: pointer;
        }
        .node text {
            fill: var(--vscode-foreground, #cccccc);
            font-size: 12px;
            pointer-events: none;
        }
        .link {
            stroke-opacity: 0.6;
            stroke-width: 2px;
        }
        .node:hover circle {
            stroke-width: 3px;
        }
        #loading {
            position: absolute;
            top: 50%;
            left: 50%;
            transform: translate(-50%, -50%);
            font-size: 18px;
        }
        #error {
            position: absolute;
            top: 50%;
            left: 50%;
            transform: translate(-50%, -50%);
            color: var(--vscode-errorForeground, #f14c4c);
            text-align: center;
        }
        .node.expanding circle {
            animation: pulse 1s infinite;
        }
        @keyframes pulse {
            0% { stroke-width: 2px; }
            50% { stroke-width: 5px; }
            100% { stroke-width: 2px; }
        }
        .node.expanded circle {
            stroke: #4CAF50;
            stroke-width: 3px;
        }
    </style>
</head>
<body>
    <div id="controls">
        <button id="zoomIn">Zoom In</button>
        <button id="zoomOut">Zoom Out</button>
        <button id="resetView">Reset</button>
        <span style="margin-left: 16px; border-left: 1px solid var(--vscode-foreground); padding-left: 16px;">Export:</span>
        <button id="exportSvg">SVG</button>
        <button id="exportJson">JSON</button>
    </div>
    <div id="root">
        <div id="loading">Loading graph...</div>
    </div>
    <div id="legend"></div>
    <script nonce="${nonce}">
        // Minimal D3 force simulation implementation
        // In production, this would use the full D3 library

        const vscode = acquireVsCodeApi();
        let currentData = null;
        let simulation = null;
        let svg = null;
        let g = null;
        let zoom = null;
        let transform = { x: 0, y: 0, k: 1 };

        // Language colors
        const languageColors = {
            python: '#3776AB',
            rust: '#DEA584',
            typescript: '#3178C6',
            javascript: '#F7DF1E',
            go: '#00ADD8',
        };

        // Type colors
        const typeColors = {
            module: '#4CAF50',
            file: '#2196F3',
            package: '#9C27B0',
            function: '#FF9800',
        };

        // Edge colors
        const edgeColors = {
            import: '#4CAF50',
            call: '#2196F3',
            require: '#FF9800',
            use: '#9C27B0',
            recursive: '#f14c4c',
        };

        function getNodeColor(node) {
            if (node.language && languageColors[node.language]) {
                return languageColors[node.language];
            }
            return typeColors[node.type] || '#9E9E9E';
        }

        function getNodeRadius(node) {
            const radii = {
                module: 25,
                file: 20,
                package: 22,
                function: 15,
            };
            return radii[node.type] || 15;
        }

        function getEdgeColor(edge) {
            return edgeColors[edge.type] || '#999';
        }

        function initSvg() {
            const root = document.getElementById('root');
            root.innerHTML = '';

            svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
            svg.setAttribute('width', '100%');
            svg.setAttribute('height', '100%');
            root.appendChild(svg);

            // Create defs for arrow markers
            const defs = document.createElementNS('http://www.w3.org/2000/svg', 'defs');
            Object.entries(edgeColors).forEach(([type, color]) => {
                const marker = document.createElementNS('http://www.w3.org/2000/svg', 'marker');
                marker.setAttribute('id', 'arrow-' + type);
                marker.setAttribute('viewBox', '0 -5 10 10');
                marker.setAttribute('refX', 25);
                marker.setAttribute('refY', 0);
                marker.setAttribute('markerWidth', 6);
                marker.setAttribute('markerHeight', 6);
                marker.setAttribute('orient', 'auto');

                const path = document.createElementNS('http://www.w3.org/2000/svg', 'path');
                path.setAttribute('d', 'M0,-5L10,0L0,5');
                path.setAttribute('fill', color);
                marker.appendChild(path);
                defs.appendChild(marker);
            });
            svg.appendChild(defs);

            g = document.createElementNS('http://www.w3.org/2000/svg', 'g');
            svg.appendChild(g);

            // Pan and zoom with mouse
            let isPanning = false;
            let startX, startY;

            svg.addEventListener('mousedown', (e) => {
                if (e.target === svg || e.target === g) {
                    isPanning = true;
                    startX = e.clientX - transform.x;
                    startY = e.clientY - transform.y;
                }
            });

            svg.addEventListener('mousemove', (e) => {
                if (isPanning) {
                    transform.x = e.clientX - startX;
                    transform.y = e.clientY - startY;
                    updateTransform();
                }
            });

            svg.addEventListener('mouseup', () => {
                isPanning = false;
            });

            svg.addEventListener('wheel', (e) => {
                e.preventDefault();
                const delta = e.deltaY > 0 ? 0.9 : 1.1;
                transform.k = Math.max(0.1, Math.min(4, transform.k * delta));
                updateTransform();
            });
        }

        function updateTransform() {
            g.setAttribute('transform',
                'translate(' + transform.x + ',' + transform.y + ') scale(' + transform.k + ')');
        }

        function renderGraph(data, graphType) {
            currentData = data;
            currentData.graphType = graphType;
            initSvg();

            if (!data.nodes || data.nodes.length === 0) {
                document.getElementById('root').innerHTML =
                    '<div id="error">No data to display</div>';
                return;
            }

            const width = svg.clientWidth || window.innerWidth;
            const height = svg.clientHeight || window.innerHeight;

            // Create node map
            const nodeMap = new Map();
            data.nodes.forEach(n => {
                nodeMap.set(n.id, {
                    ...n,
                    x: Math.random() * width,
                    y: Math.random() * height,
                    vx: 0,
                    vy: 0,
                });
            });

            // Create links
            const links = data.edges.map(e => ({
                ...e,
                source: nodeMap.get(e.from),
                target: nodeMap.get(e.to),
            })).filter(l => l.source && l.target);

            // Draw edges
            const linkGroup = document.createElementNS('http://www.w3.org/2000/svg', 'g');
            links.forEach(link => {
                const line = document.createElementNS('http://www.w3.org/2000/svg', 'line');
                line.setAttribute('class', 'link');
                line.setAttribute('stroke', getEdgeColor(link));
                line.setAttribute('marker-end', 'url(#arrow-' + link.type + ')');
                line._link = link;
                linkGroup.appendChild(line);
            });
            g.appendChild(linkGroup);

            // Draw nodes
            const nodeGroup = document.createElementNS('http://www.w3.org/2000/svg', 'g');
            nodeMap.forEach((node, id) => {
                const nodeEl = document.createElementNS('http://www.w3.org/2000/svg', 'g');
                nodeEl.setAttribute('class', 'node');
                nodeEl.setAttribute('data-id', id);
                nodeEl._node = node;

                const circle = document.createElementNS('http://www.w3.org/2000/svg', 'circle');
                circle.setAttribute('r', getNodeRadius(node));
                circle.setAttribute('fill', getNodeColor(node));
                nodeEl.appendChild(circle);

                const text = document.createElementNS('http://www.w3.org/2000/svg', 'text');
                text.setAttribute('x', 0);
                text.setAttribute('y', getNodeRadius(node) + 15);
                text.setAttribute('text-anchor', 'middle');
                const label = node.label.length > 20
                    ? node.label.substring(0, 17) + '...'
                    : node.label;
                text.textContent = label;
                nodeEl.appendChild(text);

                // Click handler
                nodeEl.addEventListener('click', () => {
                    vscode.postMessage({ command: 'nodeClick', nodeId: id });
                });

                // Double click to expand
                nodeEl.addEventListener('dblclick', () => {
                    vscode.postMessage({ command: 'expandNode', nodeId: id });
                });

                // Drag handling
                let isDragging = false;
                let dragStartX, dragStartY;

                nodeEl.addEventListener('mousedown', (e) => {
                    e.stopPropagation();
                    isDragging = true;
                    dragStartX = e.clientX;
                    dragStartY = e.clientY;
                    node.fx = node.x;
                    node.fy = node.y;
                });

                document.addEventListener('mousemove', (e) => {
                    if (isDragging) {
                        const dx = (e.clientX - dragStartX) / transform.k;
                        const dy = (e.clientY - dragStartY) / transform.k;
                        node.fx = node.x + dx;
                        node.fy = node.y + dy;
                        dragStartX = e.clientX;
                        dragStartY = e.clientY;
                        node.x = node.fx;
                        node.y = node.fy;
                        updatePositions();
                    }
                });

                document.addEventListener('mouseup', () => {
                    if (isDragging) {
                        isDragging = false;
                        node.fx = null;
                        node.fy = null;
                    }
                });

                nodeGroup.appendChild(nodeEl);
            });
            g.appendChild(nodeGroup);

            // Simple force simulation
            const nodes = Array.from(nodeMap.values());

            function tick() {
                // Apply forces
                nodes.forEach(node => {
                    // Center force
                    node.vx += (width/2 - node.x) * 0.001;
                    node.vy += (height/2 - node.y) * 0.001;

                    // Collision force
                    nodes.forEach(other => {
                        if (other === node) return;
                        const dx = node.x - other.x;
                        const dy = node.y - other.y;
                        const dist = Math.sqrt(dx*dx + dy*dy) || 1;
                        const minDist = 80;
                        if (dist < minDist) {
                            const force = (minDist - dist) / dist * 0.5;
                            node.vx += dx * force;
                            node.vy += dy * force;
                        }
                    });
                });

                // Apply link forces
                links.forEach(link => {
                    const dx = link.target.x - link.source.x;
                    const dy = link.target.y - link.source.y;
                    const dist = Math.sqrt(dx*dx + dy*dy) || 1;
                    const targetDist = 120;
                    const force = (dist - targetDist) / dist * 0.05;

                    if (!link.source.fx) {
                        link.source.vx += dx * force;
                        link.source.vy += dy * force;
                    }
                    if (!link.target.fx) {
                        link.target.vx -= dx * force;
                        link.target.vy -= dy * force;
                    }
                });

                // Update positions
                nodes.forEach(node => {
                    if (!node.fx) {
                        node.vx *= 0.8;
                        node.vy *= 0.8;
                        node.x += node.vx;
                        node.y += node.vy;
                    }
                });

                updatePositions();
            }

            function updatePositions() {
                // Update links
                linkGroup.querySelectorAll('line').forEach(line => {
                    const link = line._link;
                    line.setAttribute('x1', link.source.x);
                    line.setAttribute('y1', link.source.y);
                    line.setAttribute('x2', link.target.x);
                    line.setAttribute('y2', link.target.y);
                });

                // Update nodes
                nodeGroup.querySelectorAll('.node').forEach(nodeEl => {
                    const node = nodeEl._node;
                    nodeEl.setAttribute('transform', 'translate(' + node.x + ',' + node.y + ')');
                });
            }

            // Run simulation
            let tickCount = 0;
            function animate() {
                if (tickCount < 300) {
                    tick();
                    tickCount++;
                    requestAnimationFrame(animate);
                }
            }
            animate();

            // Update legend
            updateLegend(graphType);
        }

        function updateLegend(graphType) {
            const legend = document.getElementById('legend');
            legend.innerHTML = '<strong>Legend</strong>';

            if (graphType === 'dependency') {
                Object.entries(languageColors).forEach(([lang, color]) => {
                    const item = document.createElement('div');
                    item.className = 'legend-item';
                    item.innerHTML = '<div class="legend-color" style="background:' + color + '"></div>' + lang;
                    legend.appendChild(item);
                });
            } else {
                const item = document.createElement('div');
                item.className = 'legend-item';
                item.innerHTML = '<div class="legend-color" style="background:#FF9800"></div>Function';
                legend.appendChild(item);
            }
        }

        // Track expanding nodes
        let expandingNodes = new Set();

        function mergeGraphData(existing, newData) {
            // Create maps for deduplication
            const nodeMap = new Map();
            existing.nodes.forEach(n => nodeMap.set(n.id, n));
            newData.nodes.forEach(n => {
                if (!nodeMap.has(n.id)) {
                    nodeMap.set(n.id, n);
                }
            });

            const edgeSet = new Set();
            const edges = [];

            function edgeKey(e) {
                return e.from + '->' + e.to + ':' + e.type;
            }

            existing.edges.forEach(e => {
                const key = edgeKey(e);
                if (!edgeSet.has(key)) {
                    edgeSet.add(key);
                    edges.push(e);
                }
            });

            newData.edges.forEach(e => {
                const key = edgeKey(e);
                if (!edgeSet.has(key)) {
                    edgeSet.add(key);
                    edges.push(e);
                }
            });

            return {
                nodes: Array.from(nodeMap.values()),
                edges: edges
            };
        }

        // Message handling
        window.addEventListener('message', event => {
            const message = event.data;
            switch (message.command) {
                case 'renderGraph':
                    renderGraph(message.data, message.graphType);
                    break;
                case 'expanding':
                    // Show loading indicator on the node
                    expandingNodes.add(message.nodeId);
                    const nodeEl = document.querySelector('.node[data-id="' + message.nodeId + '"]');
                    if (nodeEl) {
                        nodeEl.classList.add('expanding');
                    }
                    break;
                case 'expandComplete':
                    expandingNodes.delete(message.nodeId);
                    const expandedNode = document.querySelector('.node[data-id="' + message.nodeId + '"]');
                    if (expandedNode) {
                        expandedNode.classList.remove('expanding');
                    }
                    if (message.success && message.data && currentData) {
                        // Merge new data with existing and re-render
                        const mergedData = mergeGraphData(currentData, message.data);
                        renderGraph(mergedData, currentData.graphType || 'dependency');
                    }
                    break;
                case 'error':
                    document.getElementById('root').innerHTML =
                        '<div id="error">' + message.message + '</div>';
                    break;
            }
        });

        // Button handlers
        document.getElementById('zoomIn').addEventListener('click', () => {
            transform.k = Math.min(4, transform.k * 1.2);
            updateTransform();
        });

        document.getElementById('zoomOut').addEventListener('click', () => {
            transform.k = Math.max(0.1, transform.k * 0.8);
            updateTransform();
        });

        document.getElementById('resetView').addEventListener('click', () => {
            transform = { x: 0, y: 0, k: 1 };
            updateTransform();
        });

        document.getElementById('exportSvg').addEventListener('click', () => {
            if (!svg) {
                return;
            }
            // Clone SVG to add proper styling for export
            const svgClone = svg.cloneNode(true);
            svgClone.setAttribute('xmlns', 'http://www.w3.org/2000/svg');

            // Add inline styles for export
            const style = document.createElementNS('http://www.w3.org/2000/svg', 'style');
            style.textContent = \`
                .node circle { stroke: #fff; stroke-width: 2px; }
                .node text { fill: #ccc; font-size: 12px; font-family: sans-serif; }
                .link { stroke-opacity: 0.6; stroke-width: 2px; }
                text { fill: #ccc; }
            \`;
            svgClone.insertBefore(style, svgClone.firstChild);

            // Add background
            const rect = document.createElementNS('http://www.w3.org/2000/svg', 'rect');
            rect.setAttribute('width', '100%');
            rect.setAttribute('height', '100%');
            rect.setAttribute('fill', '#1e1e1e');
            svgClone.insertBefore(rect, svgClone.firstChild);

            const svgContent = new XMLSerializer().serializeToString(svgClone);
            vscode.postMessage({
                command: 'exportSvg',
                svgContent: '<?xml version="1.0" encoding="UTF-8"?>\\n' + svgContent
            });
        });

        document.getElementById('exportJson').addEventListener('click', () => {
            if (!currentData) {
                return;
            }
            vscode.postMessage({
                command: 'exportJson',
                data: currentData
            });
        });
    </script>
</body>
</html>`;
    }

    private dispose(): void {
        GraphVisualizationPanel.currentPanel = undefined;
        this.panel.dispose();
        while (this.disposables.length) {
            const x = this.disposables.pop();
            if (x) {
                x.dispose();
            }
        }
    }
}

function getNonce(): string {
    let text = '';
    const possible = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';
    for (let i = 0; i < 32; i++) {
        text += possible.charAt(Math.floor(Math.random() * possible.length));
    }
    return text;
}
