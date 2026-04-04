// Copyright 2025-2026 Andrey Vasilevsky <anvanster@gmail.com>
// SPDX-License-Identifier: Apache-2.0

import * as vscode from 'vscode';
import { LanguageClient, RequestType } from 'vscode-languageclient/node';
import { AIContextParams, AIContextResponse } from '../types';

// eslint-disable-next-line @typescript-eslint/no-unused-vars
namespace GetAIContextRequest {
    export const type = new RequestType<AIContextParams, AIContextResponse, void>(
        'codegraph/getAIContext'
    );
}

/**
 * Formatted AI context ready for use with AI assistants
 */
export interface AIContext {
    primary: {
        code: string;
        language: string;
        description: string;
    };
    related: Array<{
        code: string;
        relationship: string;
        relevance: number;
    }>;
    architecture?: {
        module: string;
        neighbors: string[];
    };
}

/**
 * Provides code context to AI assistants through the CodeGraph LSP.
 */
export class CodeGraphAIProvider {
    constructor(private client: LanguageClient) {}

    /**
     * Get AI-optimized code context for the given position.
     */
    async provideCodeContext(
        document: vscode.TextDocument,
        position: vscode.Position,
        intent: 'explain' | 'modify' | 'debug' | 'test'
    ): Promise<AIContext> {
        const config = vscode.workspace.getConfiguration('codegraph');
        const maxTokens = config.get<number>('ai.maxContextTokens', 4000);

        const response = await this.client.sendRequest('workspace/executeCommand', {
            command: 'codegraph.getAIContext',
            arguments: [{
                uri: document.uri.toString(),
                position: {
                    line: position.line,
                    character: position.character,
                },
                contextType: intent,
                maxTokens,
            }]
        }) as AIContextResponse;

        return this.formatForAI(response);
    }

    /**
     * Build an enhanced prompt with code context.
     */
    buildEnhancedPrompt(userMessage: string, context: AIContext): string {
        return `
You are analyzing code with the following context:

## Primary Code
\`\`\`${context.primary.language}
${context.primary.code}
\`\`\`

${context.related.length > 0 ? `
## Related Code
${context.related.slice(0, 5).map(r => `
### ${r.relationship} (relevance: ${(r.relevance * 100).toFixed(0)}%)
\`\`\`
${r.code}
\`\`\`
`).join('\n')}
` : ''}

${context.architecture ? `
## Architecture Context
- Module: ${context.architecture.module}
- Neighbors: ${context.architecture.neighbors.join(', ')}
` : ''}

## User Question
${userMessage}
`;
    }

    /**
     * Format the LSP response for AI consumption.
     */
    private formatForAI(response: AIContextResponse): AIContext {
        return {
            primary: {
                code: response.primaryContext.code,
                language: response.primaryContext.language,
                description: `${response.primaryContext.type}: ${response.primaryContext.name}`,
            },
            related: response.relatedSymbols.map(s => ({
                code: s.code,
                relationship: s.relationship,
                relevance: s.relevanceScore,
            })),
            architecture: response.architecture,
        };
    }
}
