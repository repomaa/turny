import type { WsEvent } from './types';

type Handler = (event: WsEvent) => void;

export class WebSocketManager {
	private ws: WebSocket | null = null;
	private url: string;
	private handlers: Set<Handler> = new Set();
	private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
	private reconnectDelay = 1000;
	private maxReconnectDelay = 10000;
	private closed = false;

	constructor(url?: string) {
		if (url) {
			this.url = url;
		} else {
			const proto = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
			this.url = `${proto}//${window.location.host}/ws`;
		}
	}

	connect(): void {
		this.closed = false;
		this.doConnect();
	}

	private doConnect(): void {
		this.ws = new WebSocket(this.url);

		this.ws.onopen = () => {
			this.reconnectDelay = 1000;
		};

		this.ws.onmessage = (e: MessageEvent) => {
			try {
				const data = JSON.parse(e.data) as WsEvent;
				this.handlers.forEach((h) => h(data));
			} catch {
				// ignore malformed messages
			}
		};

		this.ws.onclose = () => {
			this.ws = null;
			if (!this.closed) {
				this.scheduleReconnect();
			}
		};

		this.ws.onerror = () => {
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
	}
}
