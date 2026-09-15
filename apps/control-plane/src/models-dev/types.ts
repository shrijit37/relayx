/**
 * models.dev catalog types (control-plane).
 *
 * Mirrors the JSON shapes from https://models.dev/api.json.
 * The control-plane owns the network fetch; the data-plane sees a
 * pre-validated snapshot — never raw network.
 */

export interface ModelModalities {
  input: string[];
  output: string[];
}

export interface ModelLimit {
  context: number;
  output: number;
}

export interface ModelCost {
  input: number;
  output: number;
  cache_read?: number;
}

export interface ModelDef {
  id: string;
  name: string;
  description: string;
  family?: string;
  attachment: boolean;
  reasoning: boolean;
  structured_output?: boolean;
  temperature: boolean;
  tool_call: boolean;
  release_date?: string;
  last_updated?: string;
  knowledge?: string;
  modalities: ModelModalities;
  open_weights: boolean;
  limit?: ModelLimit;
  cost?: ModelCost;
}

/** Provider entry from models.dev/api.json (the canonical source). */
export interface ProviderCatalogEntry {
  id: string;
  name: string;
  api?: string;
  doc?: string;
  env?: string[];
  npm?: string;
  models: Record<string, ModelDef>;
}

/** Flat model list keyed by provider/model id (e.g. "openai/gpt-4o"). */
export type ModelCatalog = Record<string, ModelDef>;

/** Metadata returned by the sync loop and the /catalog/status endpoint. */
export interface CatalogMeta {
  version: number;
  last_sync: string;
  source: string;
  model_count: number;
  provider_count: number;
}
