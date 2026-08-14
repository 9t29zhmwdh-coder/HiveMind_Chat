export type TurnPolicy = "parallel" | "round_robin" | "debate" | "moderated" | "consensus";

export interface TokenUsage {
  input_tokens: number;
  output_tokens: number;
}

export interface Agent {
  id: string;
  name: string;
  provider_id: string;
  model: string;
  persona: string;
  temperature: number;
  max_tokens: number;
  colour: string;
  reasoning: boolean;
  enabled: boolean;
}

export interface Room {
  id: string;
  name: string;
  topic: string;
  policy: TurnPolicy;
  rounds: number;
  moderator_id?: string | null;
  agents: Agent[];
  created_at: string;
}

export interface RoomSummary {
  id: string;
  name: string;
  topic: string;
  policy: TurnPolicy;
  agents: number;
  messages: number;
  created_at: string;
}

export interface Message {
  id: string;
  room_id: string;
  role: "system" | "user" | "assistant";
  speaker: string;
  agent_id: string | null;
  content: string;
  created_at: string;
  round: number;
  usage?: TokenUsage;
}

export interface Provider {
  id: string;
  label: string;
  kind: "ollama" | "anthropic" | "openai";
  base_url: string;
  local: boolean;
  credential_env: string | null;
  credential_available: boolean;
}

export interface PolicyInfo {
  id: TurnPolicy;
  summary: string;
}

/** Events the orchestrator emits while a turn runs. */
export type SessionEvent =
  | { type: "user_message"; message: Message }
  | { type: "turn_started"; round: number; rounds: number; policy: TurnPolicy; speakers: string[] }
  | { type: "agent_started"; agent_id: string; agent_name: string; colour: string; round: number }
  | { type: "agent_delta"; agent_id: string; text: string }
  | { type: "agent_completed"; message: Message }
  | { type: "agent_failed"; agent_id: string; agent_name: string; error: string }
  | { type: "vote_cast"; agent_id: string; agent_name: string; choice: string; rationale: string }
  | { type: "turn_completed"; round: number }
  | { type: "session_completed"; messages: number; usage: TokenUsage };

export type ServerFrame =
  | { type: "ready"; room: Room; history: Message[] }
  | { type: "event"; event: SessionEvent }
  | { type: "stopped" }
  | { type: "error"; message: string };

/** A message still being streamed, before it lands in the transcript. */
export interface PendingTurn {
  agentId: string;
  agentName: string;
  colour: string;
  text: string;
}

export interface Vote {
  agentId: string;
  agentName: string;
  choice: string;
  rationale: string;
}
