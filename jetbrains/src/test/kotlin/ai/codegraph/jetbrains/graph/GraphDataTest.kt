// Copyright 2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

package ai.codegraph.jetbrains.graph

import com.google.gson.Gson
import com.google.gson.JsonParser
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The two graph commands answer with different node shapes, and the panel
 * normalises them into one. A normaliser that silently drops nodes produces an
 * empty graph that looks exactly like "this file has no relationships".
 */
class GraphDataTest {

    private val gson = Gson()

    private fun parse(json: String) = GraphData.from(JsonParser.parseString(json), gson)

    @Test
    fun `dependency graph nodes keep label, type and language`() {
        val graph = parse(
            """
            {
              "nodes": [
                {"id":"1","label":"service.py","type":"Module","language":"python","uri":"file:///a/service.py"},
                {"id":"2","label":"repo.go","type":"Module","language":"go","uri":"file:///a/repo.go"}
              ],
              "edges": [{"from":"1","to":"2","type":"imports"}]
            }
            """.trimIndent(),
        )

        assertEquals(2, graph.nodes.size)
        assertEquals("service.py", graph.nodes[0].label)
        assertEquals("python", graph.nodes[0].language)
        assertEquals("imports", graph.edges[0].type)
    }

    @Test
    fun `call graph nodes label from name instead of label`() {
        // The call graph reports FunctionNode, which has `name` and no `label`.
        // Reading only `label` would leave every node showing its raw id.
        val graph = parse(
            """
            {
              "root": {"id":"1","name":"place_order"},
              "nodes": [{"id":"1","name":"place_order"},{"id":"2","name":"save"}],
              "edges": [{"from":"1","to":"2"}]
            }
            """.trimIndent(),
        )

        assertEquals(listOf("place_order", "save"), graph.nodes.map { it.label })
        assertEquals("calls", graph.edges[0].type)
    }

    @Test
    fun `a node without an id is dropped rather than rendered as a blank`() {
        val graph = parse("""{"nodes":[{"label":"orphan"},{"id":"1","label":"real"}],"edges":[]}""")

        assertEquals(1, graph.nodes.size)
        assertEquals("real", graph.nodes[0].label)
    }

    @Test
    fun `an edge missing an endpoint is dropped`() {
        val graph = parse("""{"nodes":[{"id":"1","label":"a"}],"edges":[{"from":"1"},{"to":"1"}]}""")

        assertTrue(graph.edges.isEmpty())
    }

    @Test
    fun `a node with neither label nor name falls back to its id`() {
        val graph = parse("""{"nodes":[{"id":"node-7"}],"edges":[]}""")

        assertEquals("node-7", graph.nodes[0].label)
    }

    @Test
    fun `an empty or malformed response is empty rather than an error`() {
        assertEquals(0, GraphData.from(null, gson).nodes.size)
        assertEquals(0, parse("{}").nodes.size)
        assertEquals(0, parse("[]").nodes.size)
    }

    @Test
    fun `html escapes labels so a symbol name cannot inject markup`() {
        val graph = GraphData(
            nodes = listOf(GraphNode("1", "<img src=x onerror=alert(1)>", "Function", "python", "")),
            edges = emptyList(),
        )

        val html = GraphHtml.render(graph, "Call Graph")

        assertTrue("raw markup must not reach the page", !html.contains("<img src=x"))
    }

    @Test
    fun `a large graph is capped, keeping the most connected nodes`() {
        // The layout is an all-pairs loop run to convergence before the first
        // paint, so an uncapped hub file leaves the panel frozen with nothing
        // on screen and no way to cancel.
        val nodes = (1..500).map { GraphNode("n$it", "sym$it", "Function", "python", "") }
        val edges = (2..500).map { GraphEdge("n1", "n$it", "calls") }

        val html = GraphHtml.render(GraphData(nodes, edges), "Call Graph")

        assertEquals(200, Regex("\"id\":\"n\\d+\"").findAll(html).count())
        assertTrue("the most connected node must survive the cap", html.contains("\"sym1\""))
        assertTrue("a silently trimmed graph reads as a complete one", html.contains("of 500 nodes"))
    }

    @Test
    fun `the text fallback lists nodes and resolves edge endpoints to labels`() {
        val graph = GraphData(
            nodes = listOf(
                GraphNode("1", "place_order", "Function", "python", ""),
                GraphNode("2", "save", "Function", "python", ""),
            ),
            edges = listOf(GraphEdge("1", "2", "calls")),
        )

        val text = GraphHtml.renderText(graph, "Call Graph")

        assertTrue(text.contains("place_order"))
        assertTrue("edges must show labels, not raw ids", text.contains("place_order  ->  save"))
    }
}
