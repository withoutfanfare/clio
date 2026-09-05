// Core kinds are guidance, not a closed enum. Keep stored custom values verbatim.
export const coreMemoryKinds = [
  "note", "fact", "decision", "summary", "task", "observation", "constraint",
  "snippet", "knowledgebase", "receipt", "preference",
];

export function memoryKinds(...sources: (readonly string[])[]): string[] {
  return [...new Set([...coreMemoryKinds, ...sources.flat()].filter(Boolean))];
}
