/**
 * Type-safe build/parse for `userRbacRole.resourceIdentifier` strings.
 *
 * Format: `<type>:<id>` where `<type>` is a registered resource type (e.g. `ryxisAgent`).
 *
 * Why a registry instead of a fixed TS literal union: resource types are contributed
 * by plugins at runtime via `getRbacSeeds().resourceTypes`. We can't statically know
 * the full union at compile time. Type guards at runtime catch typos.
 *
 * See:
 * - docs/features/rbacHardening/AUDIT.md P1-6 (motivation)
 * - docs/features/rbacHardening/GUIDELINE.md §2.1 (resourceTypes)
 */

const KNOWN_RESOURCE_TYPES = new Set<string>();

const RESOURCE_TYPE_PATTERN = /^[a-zA-Z][a-zA-Z0-9]*$/;
const SEPARATOR = ':';

export interface ParsedResourceIdentifier {
  type: string;
  id: string;
}

/** Register a resource type. Idempotent. */
export function registerResourceType(type: string): void {
  if (!RESOURCE_TYPE_PATTERN.test(type)) {
    throw new Error(
      `Invalid resource type "${type}": must match ${RESOURCE_TYPE_PATTERN} (camelCase, alphanumeric)`,
    );
  }
  KNOWN_RESOURCE_TYPES.add(type);
}

/** Register multiple resource types at once. */
export function registerResourceTypes(types: readonly string[]): void {
  for (const t of types) registerResourceType(t);
}

/** Whether a string has been registered as a resource type. */
export function isKnownResourceType(type: string): boolean {
  return KNOWN_RESOURCE_TYPES.has(type);
}

/** Snapshot of registered resource types (mainly for debugging / tests). */
export function getKnownResourceTypes(): string[] {
  return [...KNOWN_RESOURCE_TYPES].sort();
}

/** Test-only: clear the registry. Do NOT use in production code paths. */
export function _clearResourceTypeRegistry(): void {
  KNOWN_RESOURCE_TYPES.clear();
}

/**
 * Build a resourceIdentifier string. Throws if `type` was not registered.
 *
 * Use empty string `''` for non-resource grants (organization or project scope).
 */
export function buildResourceIdentifier(type: string, id: string | number): string {
  if (!isKnownResourceType(type)) {
    throw new Error(
      `Unknown resource type "${type}". Register it via plugin.getRbacSeeds().resourceTypes first. `
        + `Known: [${getKnownResourceTypes().join(', ')}]`,
    );
  }
  if (id === '' || id === null || id === undefined) {
    throw new Error(`Resource id must be non-empty (type=${type})`);
  }
  return `${type}${SEPARATOR}${String(id)}`;
}

/**
 * Parse a resourceIdentifier string. Returns null if malformed or type not registered.
 *
 * - Empty string `''` (non-resource grant) returns null.
 * - Type must be registered.
 */
export function parseResourceIdentifier(identifier: string): ParsedResourceIdentifier | null {
  if (!identifier) return null;
  const idx = identifier.indexOf(SEPARATOR);
  if (idx <= 0) return null; // no separator, or starts with ':'
  const type = identifier.slice(0, idx);
  const id = identifier.slice(idx + 1);
  if (!id) return null;
  if (!isKnownResourceType(type)) return null;
  return { type, id };
}
