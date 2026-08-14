import { useCallback, useEffect, useRef, useState } from "react";
import { socketUrl, storedToken } from "./api";
import type { Message, PendingTurn, ServerFrame, SessionEvent, Room, Vote } from "./types";

export type SessionStatus = "connecting" | "open" | "closed" | "unauthorized";

export interface Session {
  status: SessionStatus;
  room: Room | null;
  messages: Message[];
  /** Turns still streaming, keyed by agent so parallel speakers stay separate. */
  pending: PendingTurn[];
  votes: Vote[];
  round: { current: number; total: number } | null;
  running: boolean;
  error: string | null;
  send: (prompt: string) => void;
  stop: () => void;
}

/**
 * Keeps one socket open for the selected room.
 *
 * Streaming turns are held outside `messages` until their `agent_completed`
 * arrives, so a partial answer is never mistaken for a stored message.
 */
export function useSession(roomId: string | null): Session {
  const socketRef = useRef<WebSocket | null>(null);
  const [status, setStatus] = useState<SessionStatus>("closed");
  const [room, setRoom] = useState<Room | null>(null);
  const [messages, setMessages] = useState<Message[]>([]);
  const [pending, setPending] = useState<PendingTurn[]>([]);
  const [votes, setVotes] = useState<Vote[]>([]);
  const [round, setRound] = useState<{ current: number; total: number } | null>(null);
  const [running, setRunning] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!roomId) {
      setStatus("closed");
      setRoom(null);
      setMessages([]);
      return;
    }

    let closedByEffect = false;
    setStatus("connecting");
    setError(null);
    setPending([]);
    setVotes([]);
    setRound(null);
    setRunning(false);

    const socket = new WebSocket(socketUrl(roomId));
    socketRef.current = socket;

    socket.onopen = () => {
      const token = storedToken();
      if (token) {
        socket.send(JSON.stringify({ type: "auth", token }));
      }
      setStatus("open");
    };

    socket.onmessage = (raw) => {
      const frame = JSON.parse(raw.data as string) as ServerFrame;
      handleFrame(frame, { setRoom, setMessages, setPending, setVotes, setRound, setRunning, setError, setStatus });
    };

    socket.onclose = () => {
      if (!closedByEffect) {
        setStatus((current) => (current === "unauthorized" ? current : "closed"));
        setRunning(false);
      }
    };

    return () => {
      closedByEffect = true;
      socket.close();
      socketRef.current = null;
    };
  }, [roomId]);

  const send = useCallback((prompt: string) => {
    const socket = socketRef.current;
    if (!socket || socket.readyState !== WebSocket.OPEN || !prompt.trim()) {
      return;
    }
    setError(null);
    setVotes([]);
    setRunning(true);
    socket.send(JSON.stringify({ type: "prompt", text: prompt }));
  }, []);

  const stop = useCallback(() => {
    socketRef.current?.send(JSON.stringify({ type: "stop" }));
  }, []);

  return { status, room, messages, pending, votes, round, running, error, send, stop };
}

interface Handlers {
  setRoom: (room: Room) => void;
  setMessages: React.Dispatch<React.SetStateAction<Message[]>>;
  setPending: React.Dispatch<React.SetStateAction<PendingTurn[]>>;
  setVotes: React.Dispatch<React.SetStateAction<Vote[]>>;
  setRound: (round: { current: number; total: number } | null) => void;
  setRunning: (running: boolean) => void;
  setError: (message: string | null) => void;
  setStatus: (status: SessionStatus) => void;
}

function handleFrame(frame: ServerFrame, h: Handlers): void {
  if (frame.type === "ready") {
    h.setRoom(frame.room);
    h.setMessages(frame.history);
    return;
  }
  if (frame.type === "error") {
    h.setError(frame.message);
    h.setRunning(false);
    if (frame.message.includes("access token")) {
      h.setStatus("unauthorized");
    }
    return;
  }
  if (frame.type === "stopped") {
    h.setRunning(false);
    h.setPending([]);
    return;
  }
  handleEvent(frame.event, h);
}

function handleEvent(event: SessionEvent, h: Handlers): void {
  switch (event.type) {
    case "user_message":
      h.setMessages((current) => [...current, event.message]);
      break;
    case "turn_started":
      h.setRound({ current: event.round, total: event.rounds });
      break;
    case "agent_started":
      h.setPending((current) => [
        ...current,
        { agentId: event.agent_id, agentName: event.agent_name, colour: event.colour, text: "" },
      ]);
      break;
    case "agent_delta":
      h.setPending((current) =>
        current.map((turn) => (turn.agentId === event.agent_id ? { ...turn, text: turn.text + event.text } : turn)),
      );
      break;
    case "agent_completed":
      h.setPending((current) => current.filter((turn) => turn.agentId !== event.message.agent_id));
      h.setMessages((current) => [...current, event.message]);
      break;
    case "agent_failed":
      h.setPending((current) => current.filter((turn) => turn.agentId !== event.agent_id));
      h.setError(`${event.agent_name}: ${event.error}`);
      break;
    case "vote_cast":
      h.setVotes((current) => [
        ...current,
        { agentId: event.agent_id, agentName: event.agent_name, choice: event.choice, rationale: event.rationale },
      ]);
      break;
    case "turn_completed":
      break;
    case "session_completed":
      h.setRunning(false);
      h.setRound(null);
      h.setPending([]);
      break;
  }
}
