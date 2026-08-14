import type { Message, PolicyInfo, Provider, Room, RoomSummary } from "./types";

const TOKEN_KEY = "hivemind.token";

/** Raised when the server rejects the stored token, so the UI can re-prompt. */
export class UnauthorizedError extends Error {
  constructor() {
    super("unauthorized");
    this.name = "UnauthorizedError";
  }
}

export function storedToken(): string {
  return localStorage.getItem(TOKEN_KEY) ?? "";
}

export function storeToken(token: string): void {
  if (token) {
    localStorage.setItem(TOKEN_KEY, token);
  } else {
    localStorage.removeItem(TOKEN_KEY);
  }
}

async function request<T>(path: string, init: RequestInit = {}): Promise<T> {
  const headers = new Headers(init.headers);
  const token = storedToken();
  if (token) {
    headers.set("Authorization", `Bearer ${token}`);
  }
  if (init.body !== undefined) {
    headers.set("Content-Type", "application/json");
  }

  const response = await fetch(path, { ...init, headers });
  if (response.status === 401) {
    throw new UnauthorizedError();
  }
  if (!response.ok) {
    throw new Error(await errorMessage(response));
  }
  return response.headers.get("content-type")?.includes("json")
    ? ((await response.json()) as T)
    : ((await response.text()) as unknown as T);
}

/** Prefers the server's error field and falls back to the status line. */
async function errorMessage(response: Response): Promise<string> {
  try {
    const body = (await response.json()) as { error?: string };
    if (body.error) {
      return body.error;
    }
  } catch {
    // Not a JSON body; fall through to the status text.
  }
  return `${response.status} ${response.statusText}`;
}

export interface RoomDetail extends Room {
  messages: Message[];
}

export interface AgentInput {
  id?: string;
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

export interface RoomInput {
  name: string;
  topic: string;
  policy: Room["policy"];
  rounds: number;
  moderator_id?: string | null;
  agents: AgentInput[];
}

export const api = {
  health: () => request<{ status: string; version: string }>("/api/health"),
  providers: () => request<Provider[]>("/api/providers"),
  models: (providerId: string) => request<string[]>(`/api/providers/${encodeURIComponent(providerId)}/models`),
  policies: () => request<PolicyInfo[]>("/api/policies"),
  rooms: () => request<RoomSummary[]>("/api/rooms"),
  room: (id: string) => request<RoomDetail>(`/api/rooms/${encodeURIComponent(id)}`),
  createRoom: (input: RoomInput) => request<Room>("/api/rooms", { method: "POST", body: JSON.stringify(input) }),
  updateRoom: (id: string, input: RoomInput) =>
    request<Room>(`/api/rooms/${encodeURIComponent(id)}`, { method: "PUT", body: JSON.stringify(input) }),
  deleteRoom: (id: string) => request<{ deleted: boolean }>(`/api/rooms/${encodeURIComponent(id)}`, { method: "DELETE" }),
  clearTranscript: (id: string) =>
    request<{ deleted: boolean }>(`/api/rooms/${encodeURIComponent(id)}/transcript`, { method: "DELETE" }),
  transcript: (id: string) => request<string>(`/api/rooms/${encodeURIComponent(id)}/transcript`),
};

/** Builds the socket URL for a room on the same origin the UI was served from. */
export function socketUrl(roomId: string): string {
  const protocol = window.location.protocol === "https:" ? "wss:" : "ws:";
  return `${protocol}//${window.location.host}/api/rooms/${encodeURIComponent(roomId)}/ws`;
}
