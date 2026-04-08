import { query } from "@tencent-ai/agent-sdk";

const TAG_RULES = `Tag language rules:
- Use Chinese for general concepts (e.g. "前端", "状态管理", "代码审查", "测试")
- Keep well-known technical terms in English (e.g. "React", "Git", "TDD", "GraphQL", "WebSocket")
- NEVER mix Chinese and English in one tag (e.g. "API设计" is WRONG, use "API 设计" or "接口设计")

Tag format rules:
- Each tag must be 2-4 characters/words, no long phrases
- English terms use their canonical casing (e.g. "React" not "react", "GraphQL" not "graphql")
- No duplicate meanings: do not output both "API 设计" and "接口设计", pick one`;

const PROMPTS = {
  tag_skill: `You are an AI coding skill classification expert. Based on the following skill content, generate 3-5 concise tags.

${TAG_RULES}

Tags should describe the skill's purpose, applicable tools, or technical domain. Return only a JSON array like ["React", "前端", "状态管理"], nothing else.`,

  generate_scenario_prompt: `You are an AI coding assistant scenario designer. Based on the scenario name and skills list below, write a scenario prompt (200-500 words). Describe the scenario's purpose and how the skills work together. Return only the prompt text, nothing else.`,

  create_scenario: `You are an AI coding assistant scenario planner. Based on the installed skills list below, suggest 2-5 scenario groupings. Each scenario should have: name (concise), description (one sentence), icon (one emoji), skillNames (array of skill names from the input). Return only a JSON array like [{"name":"...","description":"...","icon":"🎨","skillNames":["..."]}], nothing else.`,

  batch_tag_skills: `You are an AI coding skill classification expert. For EACH skill below, generate 3-5 concise tags.

${TAG_RULES}
- Use CONSISTENT tags across skills when they share the same concept (e.g. always "前端" not sometimes "前端开发")

Return ONLY a JSON object mapping each skill name to its tags array, like:
{"skill-name-1": ["React", "前端", "状态管理"], "skill-name-2": ["Git", "版本控制"]}
Nothing else.`,

  consolidate_tags: `You are an AI tag taxonomy expert. Given a list of existing tags from a coding skills library, consolidate them into 20-30 canonical tags by merging synonyms and overly-specific variants.

${TAG_RULES}

Rules:
- Merge synonyms into one canonical tag (e.g. "UI 审查"/"UI 规范"/"UI 优化"/"UI 重设计" → "UI 设计")
- Merge overly-specific variants into broader categories (e.g. "视觉升级"/"视觉层次"/"视觉层级"/"配色" → "视觉设计")
- Keep technology-specific tags that are genuinely distinct (e.g. "React" and "Next.js" stay separate)
- Every original tag MUST appear in exactly one mapping
- The canonical tag can be one of the original tags or a new name

Return ONLY a JSON object where keys are canonical tags and values are arrays of original tags that map to them:
{"UI 设计": ["UI 审查", "UI 规范", "UI 优化", "UI 重设计"], "React": ["React"], ...}
Nothing else.`,
};

function buildUserContent(task, payload) {
  switch (task) {
    case "tag_skill":
      return `Skill name: ${payload.skillName}\n\nSkill content:\n${payload.skillContent}`;
    case "generate_scenario_prompt":
      return `Scenario name: ${payload.scenarioName}\n\nSkills in this scenario:\n${payload.skills.map((s) => `- ${s.name}: ${s.description || "No description"}`).join("\n")}`;
    case "create_scenario": {
      const skillLines = payload.skills.map((s) => {
        const desc = (s.description || "No description").slice(0, 80);
        return `- ${s.name} [tags: ${(s.tags || []).join(", ") || "none"}]: ${desc}`;
      }).join("\n");
      const existingSection = payload.existingScenarios?.length
        ? `\n\nExisting scenarios (do NOT recreate these):\n${payload.existingScenarios.map((s) => `- ${s}`).join("\n")}`
        : "";
      return `All installed skills:\n${skillLines}${existingSection}`;
    }
    case "batch_tag_skills":
      return payload.skills
        .map((s) => `=== ${s.name} ===\n${s.content}`)
        .join("\n\n");
    case "consolidate_tags":
      return `All existing tags (${payload.tags.length} total):\n${payload.tags.join(", ")}`;
    default:
      throw new Error(`Unknown task: ${task}`);
  }
}

function extractJson(text) {
  try {
    return JSON.parse(text);
  } catch {
    const match = text.match(/(\[[\s\S]*\]|\{[\s\S]*\})/);
    if (match) {
      try {
        return JSON.parse(match[1]);
      } catch {
        // ignore
      }
    }
  }
  return null;
}

/** Normalize a tag for dedup comparison: lowercase, remove all spaces */
function normalizeTag(tag) {
  return tag.toLowerCase().replace(/\s+/g, "");
}

/** Deduplicate tags by normalized form, keeping the first occurrence */
function deduplicateTags(tags) {
  const seen = new Set();
  return tags.filter((tag) => {
    const key = normalizeTag(tag);
    if (seen.has(key)) return false;
    seen.add(key);
    return true;
  });
}

function formatResult(task, rawText) {
  switch (task) {
    case "tag_skill": {
      const parsed = extractJson(rawText);
      if (Array.isArray(parsed)) return { tags: deduplicateTags(parsed) };
      return null;
    }
    case "generate_scenario_prompt":
      return { prompt: rawText.trim() };
    case "create_scenario": {
      const parsed = extractJson(rawText);
      if (Array.isArray(parsed)) return { scenarios: parsed };
      return null;
    }
    case "batch_tag_skills": {
      const parsed = extractJson(rawText);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        const results = {};
        for (const [name, tags] of Object.entries(parsed)) {
          if (Array.isArray(tags)) {
            results[name] = deduplicateTags(tags);
          }
        }
        return { results };
      }
      return null;
    }
    case "consolidate_tags": {
      const parsed = extractJson(rawText);
      if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
        return { mapping: parsed };
      }
      return null;
    }
    default:
      return null;
  }
}

async function main() {
  let inputData = "";
  for await (const chunk of process.stdin) {
    inputData += chunk;
  }

  let input;
  try {
    input = JSON.parse(inputData);
  } catch {
    console.log(JSON.stringify({ ok: false, error: "Invalid JSON input" }));
    process.exit(1);
  }

  const { task, apiKey, payload } = input;

  if (!PROMPTS[task]) {
    console.log(JSON.stringify({ ok: false, error: `Unknown task: ${task}` }));
    process.exit(1);
  }

  const systemPrompt = PROMPTS[task];
  const userContent = buildUserContent(task, payload);
  const fullPrompt = `${systemPrompt}\n\n${userContent}`;

  try {
    const envVars = { CODEBUDDY_API_KEY: apiKey };

    const maxTurns = (task === "batch_tag_skills" || task === "consolidate_tags" || task === "create_scenario") ? 3 : 1;

    let resultText = "";
    const q = query({
      prompt: fullPrompt,
      options: {
        permissionMode: "bypassPermissions",
        maxTurns,
        env: envVars,
      },
    });

    for await (const message of q) {
      if (message.type === "assistant") {
        for (const block of message.message.content) {
          if (block.type === "text") {
            resultText += block.text;
          }
        }
      }
    }

    const data = formatResult(task, resultText);
    if (data) {
      console.log(JSON.stringify({ ok: true, data }));
    } else {
      console.log(
        JSON.stringify({ ok: false, error: "Failed to parse AI response", raw: resultText })
      );
    }
  } catch (err) {
    console.log(JSON.stringify({ ok: false, error: err.message || String(err) }));
    process.exit(1);
  }
}

main();
