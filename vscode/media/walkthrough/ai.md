# Give your AI assistant the graph

CodeGraph registers a set of **language-model tools**, so an AI assistant in
your editor can query the graph directly - callers, dependencies, impact,
related tests, and curated context - instead of guessing from a few open files.

Ask your assistant things like:

- "What breaks if I change the signature of `parseConfig`?"
- "Show me the tests related to this module."
- "What are the entry points into this service?"

It answers from your actual code graph, grounded in the index you just built.

No setup needed - the tools are available as soon as your workspace is indexed.
