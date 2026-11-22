---
name: rag-system-design
description: Design, evaluate, and operate retrieval-augmented generation systems
difficulty: staff
domains: [ai-ml]
---

## Overview

RAG systems fail in ways that are hard to see: wrong chunks retrieved, context window overflows, hallucinations on retrieved content, stale knowledge base. This skill builds RAG systems with rigorous evaluation at every layer.

## When to Use

- Before building any system that grounds LLM responses in external data
- When an existing RAG system is returning low-quality or inconsistent results
- Before selecting an embedding model or vector database

## Process

### Step 1: Define the retrieval task
What questions will users ask? What documents will answer them? What does "good retrieval" look like? Write 50+ question-answer pairs as your evaluation set before writing any code.

### Step 2: Design the ingestion pipeline
- Source: what documents? In what format? How frequently updated?
- Chunking strategy: fixed-size vs semantic vs structural
- Chunk size: smaller chunks = more precise retrieval; larger = more context per chunk
- Overlap: prevent context from being cut at chunk boundaries
- Metadata: document source, date, section header — critical for filtering and attribution

### Step 3: Choose and evaluate the embedding model
Test at least 2 embedding models on your retrieval evaluation set. Measure: retrieval accuracy (does the correct chunk appear in top-k?), latency, cost. Don't default to the most popular model.

### Step 4: Build the vector store
- Choose an index type appropriate for scale (exact vs approximate)
- Store metadata alongside vectors
- Plan for updates and deletions (not all vector DBs handle this well)
- Test retrieval latency at your expected query volume

### Step 5: Implement retrieval with evaluation
Measure:
- **Recall@k**: Does the correct document appear in the top k results?
- **Precision@k**: Of the top k results, how many are relevant?
- **MRR**: Mean Reciprocal Rank — how highly is the correct document ranked?

Target: Recall@5 > 0.80 on your evaluation set before moving to generation.

### Step 6: Design the context assembly
How do you combine retrieved chunks into a prompt context?
- Order matters: LLMs attend more to beginning and end
- Deduplication: don't include the same content twice
- Relevance threshold: don't include low-score chunks just to fill context
- Citation tracking: which chunks supported which claims?

### Step 7: Implement generation with attribution
The LLM should cite sources. Implement structured output that includes: answer, confidence, source chunks used. Validate that answers are grounded in retrieved content, not model memory.

### Step 8: End-to-end evaluation
Measure:
- **Faithfulness**: Is the answer supported by the retrieved context?
- **Answer relevance**: Does the answer address the question?
- **Context relevance**: Was the retrieved context actually useful?

Use an LLM-as-judge framework (RAGAS or similar) for automated evaluation.

### Step 9: Implement freshness and staleness handling
When was the source document last updated? Is the retrieved content stale? Add metadata filtering for date, version, or source. Tell users when information may be outdated.

### Step 10: Monitor production retrieval quality
Track: retrieval latency, retrieval scores over time, user feedback, answer quality. Stale embeddings (from updated documents) will silently degrade quality.

## Anti-Rationalizations

**"If retrieval finds relevant documents, the generation will be good"**
Good retrieval is necessary but not sufficient. LLMs hallucinate on retrieved content, misread it, and fail to synthesize across chunks.

**"Chunk size doesn't matter much"**
Chunk size is the most important RAG hyperparameter. Too small: no context. Too large: diluted signal, context window pressure.

## Red Flags

- No retrieval evaluation dataset
- No measurement of Recall@k
- No citation/attribution in outputs
- No monitoring for retrieval quality degradation
- Knowledge base never refreshed

## Verification Requirements

- [ ] Retrieval evaluation set of 50+ examples created before building
- [ ] Embedding model chosen based on evaluation (not defaults)
- [ ] Recall@5 > 0.80 on evaluation set
- [ ] Context assembly strategy documented
- [ ] Generation includes source attribution
- [ ] End-to-end evaluation with faithfulness metric
- [ ] Knowledge base refresh schedule defined
- [ ] Production monitoring in place
