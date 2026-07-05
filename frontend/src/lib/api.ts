import type {
	AuthUrlResponse,
	AuthStatus,
	Card,
	Playlist,
	NowPlaying,
	PlayerState,
	LastCard
} from './types';

async function request<T>(url: string, init?: RequestInit): Promise<T> {
	const res = await fetch(url, init);
	if (!res.ok) {
		throw new Error(`${url}: ${res.status} ${res.statusText}`);
	}
	const text = await res.text();
	return text ? (JSON.parse(text) as T) : (undefined as T);
}

export function getAuthUrl(): Promise<AuthUrlResponse> {
	return request<AuthUrlResponse>('/api/auth/url');
}

export function getAuthStatus(): Promise<AuthStatus> {
	return request<AuthStatus>('/api/auth/status');
}

export function logout(): Promise<void> {
	return request<void>('/api/auth/logout', { method: 'POST' });
}

export function getCards(): Promise<Card[]> {
	return request<Card[]>('/api/cards');
}

export function saveCard(card_id: string, playlist_uri: string): Promise<void> {
	return request<void>('/api/cards', {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ card_id, playlist_uri })
	});
}

export function deleteCard(card_id: string): Promise<void> {
	return request<void>(`/api/cards/${encodeURIComponent(card_id)}`, {
		method: 'DELETE'
	});
}

export function getPlaylists(): Promise<Playlist[]> {
	return request<Playlist[]>('/api/playlists');
}

export function getNowPlaying(): Promise<NowPlaying | null> {
	return request<NowPlaying | null>('/api/now-playing');
}

export function getState(): Promise<PlayerState> {
	return request<PlayerState>('/api/state');
}

export function getLastCard(): Promise<LastCard | null> {
	return request<LastCard | null>('/api/last-card');
}

export function playerPlay(): Promise<void> {
	return request<void>('/api/player/play', { method: 'POST' });
}

export function playerPause(): Promise<void> {
	return request<void>('/api/player/pause', { method: 'POST' });
}

export function playerNext(): Promise<void> {
	return request<void>('/api/player/next', { method: 'POST' });
}

export function playerPrevious(): Promise<void> {
	return request<void>('/api/player/previous', { method: 'POST' });
}
