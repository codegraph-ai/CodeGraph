// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.graph

import com.google.gson.Gson
import com.intellij.ui.JBColor
import com.intellij.util.ui.UIUtil

/**
 * Builds the graph view.
 *
 * The page is fully self-contained: the layout is a small force simulation in
 * plain JavaScript with SVG output, and nothing is fetched. A CDN script would
 * be simpler to write and would fail on exactly the machines that most need
 * this to work - offline, air-gapped, or behind a proxy that blocks it.
 *
 * Language colours match the VS Code client so the two views of the same graph
 * read the same way.
 */
object GraphHtml {

    private val gson = Gson()

    private val LANGUAGE_COLORS = mapOf(
        "typescript" to "#3178C6",
        "javascript" to "#F7DF1E",
        "python" to "#3572A5",
        "rust" to "#DEA584",
        "go" to "#00ADD8",
        "java" to "#B07219",
        "kotlin" to "#A97BFF",
        "csharp" to "#178600",
        "cpp" to "#F34B7D",
        "c" to "#555555",
        "ruby" to "#701516",
        "php" to "#4F5D95",
        "swift" to "#F05138",
        "scala" to "#C22D40",
    )

    private const val DEFAULT_COLOR = "#888888"

    fun render(graph: GraphData, title: String): String {
        val payload = gson.toJson(
            mapOf(
                "nodes" to graph.nodes.map { node ->
                    mapOf(
                        "id" to node.id,
                        "label" to node.label,
                        "color" to (LANGUAGE_COLORS[node.language.lowercase()] ?: DEFAULT_COLOR),
                        "title" to "${node.label}\n${node.type}${if (node.language.isNotBlank()) " · ${node.language}" else ""}",
                    )
                },
                "edges" to graph.edges.map { mapOf("from" to it.from, "to" to it.to) },
            ),
        )

        // The page inherits the IDE's theme rather than picking its own, so a
        // graph opened in a dark IDE is not a white rectangle.
        val background = hex(UIUtil.getPanelBackground())
        val foreground = hex(JBColor.foreground())

        return """
            <!DOCTYPE html>
            <html lang="en">
            <head>
              <meta charset="utf-8">
              <title>$title</title>
              <style>
                html, body { margin: 0; height: 100%; background: $background; color: $foreground;
                             font-family: -apple-system, "Segoe UI", sans-serif; overflow: hidden; }
                #empty { padding: 24px; font-size: 13px; opacity: 0.7; }
                svg { width: 100%; height: 100%; display: block; cursor: grab; }
                line { stroke: $foreground; stroke-opacity: 0.25; }
                circle { stroke: $background; stroke-width: 1.5px; cursor: pointer; }
                text { font-size: 11px; fill: $foreground; pointer-events: none; }
              </style>
            </head>
            <body>
              <div id="empty" hidden>No relationships to draw.</div>
              <svg id="canvas"></svg>
              <script>
                const data = $payload;
                const svg = document.getElementById('canvas');
                if (!data.nodes.length) {
                  document.getElementById('empty').hidden = false;
                  svg.style.display = 'none';
                } else {
                  draw();
                }

                function draw() {
                  const width = window.innerWidth, height = window.innerHeight;
                  const nodes = data.nodes.map((n, i) => ({
                    ...n,
                    // Seeded ring placement rather than random: the same graph
                    // laying out differently on every open makes it impossible
                    // to recognise.
                    x: width / 2 + Math.cos(i * 2.399) * Math.min(width, height) * 0.3,
                    y: height / 2 + Math.sin(i * 2.399) * Math.min(width, height) * 0.3,
                    vx: 0, vy: 0,
                  }));
                  const index = new Map(nodes.map(n => [n.id, n]));
                  const edges = data.edges
                    .map(e => ({ source: index.get(e.from), target: index.get(e.to) }))
                    .filter(e => e.source && e.target);

                  for (let step = 0; step < 300; step++) tick(nodes, edges, width, height);
                  paint(nodes, edges);
                }

                function tick(nodes, edges, width, height) {
                  for (const a of nodes) {
                    for (const b of nodes) {
                      if (a === b) continue;
                      let dx = a.x - b.x, dy = a.y - b.y;
                      let d2 = dx * dx + dy * dy || 0.01;
                      const force = 900 / d2;
                      a.vx += dx * force; a.vy += dy * force;
                    }
                  }
                  for (const e of edges) {
                    const dx = e.target.x - e.source.x, dy = e.target.y - e.source.y;
                    const d = Math.sqrt(dx * dx + dy * dy) || 0.01;
                    const force = (d - 90) * 0.01;
                    const fx = dx / d * force, fy = dy / d * force;
                    e.source.vx += fx; e.source.vy += fy;
                    e.target.vx -= fx; e.target.vy -= fy;
                  }
                  for (const n of nodes) {
                    n.vx += (width / 2 - n.x) * 0.002;
                    n.vy += (height / 2 - n.y) * 0.002;
                    n.x += (n.vx *= 0.82); n.y += (n.vy *= 0.82);
                    n.x = Math.max(30, Math.min(width - 30, n.x));
                    n.y = Math.max(20, Math.min(height - 20, n.y));
                  }
                }

                function paint(nodes, edges) {
                  const parts = [];
                  for (const e of edges) {
                    parts.push('<line x1="' + e.source.x + '" y1="' + e.source.y +
                               '" x2="' + e.target.x + '" y2="' + e.target.y + '"/>');
                  }
                  for (const n of nodes) {
                    parts.push('<circle cx="' + n.x + '" cy="' + n.y + '" r="7" fill="' + n.color +
                               '"><title>' + escapeHtml(n.title) + '</title></circle>');
                    parts.push('<text x="' + (n.x + 11) + '" y="' + (n.y + 4) + '">' +
                               escapeHtml(n.label) + '</text>');
                  }
                  svg.innerHTML = parts.join('');
                }

                function escapeHtml(text) {
                  return String(text).replace(/[&<>"']/g, c =>
                    ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c]));
                }
              </script>
            </body>
            </html>
        """.trimIndent()
    }

    /** Plain-text rendering for IDEs without JCEF. */
    fun renderText(graph: GraphData, title: String): String = buildString {
        appendLine(title)
        appendLine("=".repeat(title.length))
        appendLine()
        if (graph.nodes.isEmpty()) {
            appendLine("No relationships to show.")
            return@buildString
        }
        val byId = graph.nodes.associateBy { it.id }
        appendLine("Nodes (${graph.nodes.size})")
        graph.nodes.forEach { node ->
            appendLine("  ${node.label}  [${node.type}${if (node.language.isNotBlank()) ", ${node.language}" else ""}]")
        }
        appendLine()
        appendLine("Edges (${graph.edges.size})")
        graph.edges.forEach { edge ->
            val from = byId[edge.from]?.label ?: edge.from
            val to = byId[edge.to]?.label ?: edge.to
            appendLine("  $from  ->  $to  (${edge.type})")
        }
    }

    private fun hex(color: java.awt.Color): String = "#%02x%02x%02x".format(color.red, color.green, color.blue)
}
