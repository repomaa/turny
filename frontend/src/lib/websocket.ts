import type { WsEvent } from './types';

const INITIAL_RECONNECT_DELAY_MS = 1000;
const MAX_RECONNECT_DELAY_MS = 10000;

type Handler = (event: WsEvent) => void;

export type ConnectionState = 'disconnected' | 'connecting' | 'connected';

export class WebSocketManager {
	private ws: WebSocket | null = null;
	private url: string;
	private handlers: Set<Handler> = new Set();
	private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
	private reconnectDelay = INITIAL_RECONNECT_DELAY_MS;
	private maxReconnectDelay = MAX_RECONNECT_DELAY_MS;
	private closed = false;
	private state: ConnectionState = 'disconnected';
	private stateHandlers: Set<(state: ConnectionState) => void> = new Set();

	constructor(url?: string) {
		if (url) {
			this.url = url;
		} else {
			const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
			this.url = `${proto}//${window.location.host}/ws`;
		}
	}

	get connectionState(): ConnectionState {
		return this.state;
	}

	onStateChange(handler: (state: ConnectionState) => void): () => void {
		this.stateHandlers.add(handler);
		return () => {
			this.stateHandlers.delete(handler);
		};
	}

	private setState(s: ConnectionState) {
		this.state = s;
		this.stateHandlers.forEach((h) => h(s));
	}

	connect(): void {
		this.closed = false;
		this.reconnectDelay = INITIAL_RECONNECT_DELAY_MS;
		this.doConnect();
	}

	private doConnect(): void {
		this.setState('connecting');
		this.ws = new WebSocket(this.url);

		this.ws.onopen = () => {
			this.reconnectDelay = 1000;
			this.setState('connected');
		};

		this.ws.onmessage = (e: MessageEvent) => {
			try {
				const data = JSON.parse(e.data) as WsEvent;
				this.handlers.forEach((h) => h(data));
			} catch (e) {
				console.warn('Malformed WebSocket message:', e);
			}
		};

		this.ws.onclose = () => {
			this.ws = null;
			this.setState('disconnected');
			if (!this.closed) {
				this.scheduleReconnect();
			}
		};

		this.ws.onerror = (e) => {
			console.warn('WebSocket error:', e);
			this.ws?.close();
		};
	}

	private scheduleReconnect(): void {
		if (this.reconnectTimer) clearTimeout(this.reconnectTimer);
		this.reconnectTimer = setTimeout(() => {
			this.doConnect();
		}, this.reconnectDelay);
		this.reconnectDelay = Math.min(this.reconnectDelay * 2, this.maxReconnectDelay);
	}

	on(handler: Handler): () => void {
		this.handlers.add(handler);
		return () => {
			this.handlers.delete(handler);
		};
	}

	disconnect(): void {
		this.closed = true;
		if (this.reconnectTimer) {
			clearTimeout(this.reconnectTimer);
			this.reconnectTimer = null;
		}
		this.ws?.close();
		this.ws = null;
		this.setState('disconnected');
	}
}
