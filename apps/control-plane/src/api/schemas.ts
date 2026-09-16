/**
 * Wire DTOs + zod validation for the control-plane REST API.
 * API surfaces never include credentials — only `credential_ref` references.
 */

import { z } from "zod";

export const workflowStatusSchema = z.enum(["draft", "validated", "compiled", "published", "active"]);

export const credentialRefSchema = z.object({
  ref: z.string(),
  provider: z.enum(["env", "vault"]),
});

export const providerDtoSchema = z.object({
  name: z.string().min(1),
  protocol: z.enum(["openai_chat", "anthropic", "openai_responses"]),
  base_url: z.string().url(),
  model: z.string().min(1),
});

export const laneDtoSchema = z.object({
  id: z.string().min(1).optional(),
  name: z.string().min(1),
  project_id: z.string().min(1),
  provider_id: z.string().nullable().optional(),
  endpoint: z.string().min(1),
  base_url: z.string().url(),
  egress: z.string().default("direct"),
  policies: z.array(z.string()).default([]),
  credential_ref: credentialRefSchema.nullable().optional(),
});

export const workflowUpdateSchema = z.object({
  name: z.string().min(1).optional(),
  project_id: z.string().min(1).optional(),
});

export const createWorkflowSchema = z.object({
  id: z.string().min(1).optional(),
  name: z.string().min(1),
  project_id: z.string().min(1),
});

export const publishSchema = z.object({
  workflow_json: z.record(z.string(), z.unknown()),
  version: z.number().int().positive().optional(),
});