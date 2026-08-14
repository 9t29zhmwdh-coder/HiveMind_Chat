/**
 * Minimal two-language dictionary.
 *
 * English is the default; German is offered when the browser asks for it. The
 * choice is never hard-coded: the stored preference wins, then the browser's
 * language list, then English.
 */
const STORAGE_KEY = "hivemind.language";

export type Language = "en" | "de";

export const STRINGS = {
  en: {
    rooms: "Rooms",
    newRoom: "New room",
    noRooms: "No rooms yet. Create one to get started.",
    agents: "Agents",
    addAgent: "Add agent",
    editAgent: "Edit agent",
    name: "Name",
    provider: "Provider",
    model: "Model",
    persona: "Persona",
    personaHint: "How this agent should behave and what it should argue for.",
    temperature: "Temperature",
    maxTokens: "Max tokens",
    colour: "Colour",
    reasoning: "Let the model reason first",
    reasoningHint: "Slower and uses more tokens; reasoning is drawn from the same budget as the answer.",
    enabled: "Takes part",
    policy: "Policy",
    rounds: "Rounds",
    topic: "Topic",
    save: "Save",
    cancel: "Cancel",
    remove: "Remove",
    deleteRoom: "Delete room",
    clearTranscript: "Clear transcript",
    exportTranscript: "Export as Markdown",
    send: "Send",
    stop: "Stop",
    promptPlaceholder: "Ask the room something…",
    running: "The room is talking…",
    connecting: "Connecting…",
    disconnected: "Disconnected",
    reconnect: "Reconnect",
    votes: "Votes",
    round: "Round",
    you: "You",
    tokenTitle: "Access token required",
    tokenHint: "This instance was started with an access token. Enter it to continue.",
    tokenPlaceholder: "Access token",
    unlock: "Unlock",
    credentialMissing: "credential missing",
    localProvider: "local",
    noAgents: "This room has no agents yet. Add at least one to start talking.",
    confirmDeleteRoom: "Delete this room and its whole transcript?",
    confirmClear: "Delete every message in this room?",
    language: "Language",
    usage: "tokens",
    failed: "could not answer",
  },
  de: {
    rooms: "Räume",
    newRoom: "Neuer Raum",
    noRooms: "Noch keine Räume. Lege einen an, um zu starten.",
    agents: "Agenten",
    addAgent: "Agent hinzufügen",
    editAgent: "Agent bearbeiten",
    name: "Name",
    provider: "Anbieter",
    model: "Modell",
    persona: "Rolle",
    personaHint: "Wie sich dieser Agent verhalten und wofür er argumentieren soll.",
    temperature: "Temperatur",
    maxTokens: "Max. Tokens",
    colour: "Farbe",
    reasoning: "Modell zuerst nachdenken lassen",
    reasoningHint: "Langsamer und teurer; das Nachdenken zehrt vom selben Budget wie die Antwort.",
    enabled: "Nimmt teil",
    policy: "Ablauf",
    rounds: "Runden",
    topic: "Thema",
    save: "Speichern",
    cancel: "Abbrechen",
    remove: "Entfernen",
    deleteRoom: "Raum löschen",
    clearTranscript: "Verlauf löschen",
    exportTranscript: "Als Markdown exportieren",
    send: "Senden",
    stop: "Stopp",
    promptPlaceholder: "Frag den Raum etwas…",
    running: "Der Raum diskutiert…",
    connecting: "Verbinde…",
    disconnected: "Verbindung getrennt",
    reconnect: "Neu verbinden",
    votes: "Abstimmung",
    round: "Runde",
    you: "Du",
    tokenTitle: "Zugriffstoken nötig",
    tokenHint: "Diese Instanz wurde mit einem Zugriffstoken gestartet. Gib es ein, um fortzufahren.",
    tokenPlaceholder: "Zugriffstoken",
    unlock: "Entsperren",
    credentialMissing: "Zugangsdaten fehlen",
    localProvider: "lokal",
    noAgents: "Dieser Raum hat noch keine Agenten. Füge mindestens einen hinzu.",
    confirmDeleteRoom: "Diesen Raum und den gesamten Verlauf löschen?",
    confirmClear: "Alle Nachrichten in diesem Raum löschen?",
    language: "Sprache",
    usage: "Tokens",
    failed: "konnte nicht antworten",
  },
} as const;

/// Same keys as the English block, any string value: without the mapped type
/// `as const` would pin every value to its English literal and reject German.
export type Strings = { readonly [K in keyof (typeof STRINGS)["en"]]: string };

// Fails to compile if a translation drops or misspells a key.
const _completeness: Record<Language, Strings> = STRINGS;
void _completeness;

export function detectLanguage(): Language {
  const stored = localStorage.getItem(STORAGE_KEY);
  if (stored === "en" || stored === "de") {
    return stored;
  }
  const preferred = navigator.languages ?? [navigator.language];
  return preferred.some((tag) => tag.toLowerCase().startsWith("de")) ? "de" : "en";
}

export function storeLanguage(language: Language): void {
  localStorage.setItem(STORAGE_KEY, language);
}
