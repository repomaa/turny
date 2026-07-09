import type {
	AuthUrlResponse,
	AuthStatus,
	Card,
	Playlist,
	NowPlaying,
	Volume
} from './types';

async function request<T>(url: string, init?: RequestInit): Promise<T | undefined> {
	const res = await fetch(url, init);
	if (!res.ok) {
		const body = await res.text().catch(() => '');
		throw new Error(`${url}: ${res.status} ${res.statusText}${body ? ` — ${body}` : ''}`);
	}
	const text = await res.text();
	return text ? (JSON.parse(text) as T) : undefined;
}

export function getAuthUrl(): Promise<AuthUrlResponse> {
	return request<AuthUrlResponse>('/api/auth/url');
}

export function getAuthStatus(): Promise<AuthStatus> {
	return request<AuthStatus>('/api/auth/status');
}

export function logout(): Promise<void> {
	return request('/api/auth/logout', { method: 'POST' }).then(() => undefined);
}

export function getCards(): Promise<Card[]> {
	return request<Card[]>('/api/cards');
}

export function saveCard(card_id: string, playlist_uri: string, playlist_name?: string): Promise<void> {
	return request('/api/cards', {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ card_id, playlist_uri, playlist_name })
	}).then(() => undefined);
}

export function deleteCard(card_id: string): Promise<void> {
	return request(`/api/cards/${encodeURIComponent(card_id)}`, {
		method: 'DELETE'
	}).then(() => undefined);
}

export function getPlaylists(): Promise<Playlist[]> {
	return request<Playlist[]>('/api/playlists');
}

export function getNowPlaying(): Promise<NowPlaying | null> {
	return request<NowPlaying | null>('/api/now-playing');
}

export function playerPlay(): Promise<void> {
	return request('/api/player/play', { method: 'POST' }).then(() => undefined);
}

export function playerPause(): Promise<void> {
	return request('/api/player/pause', { method: 'POST' }).then(() => undefined);
}

export function playerNext(): Promise<void> {
	return request('/api/player/next', { method: 'POST' }).then(() => undefined);
}

export function playerPrevious(): Promise<void> {
	return request('/api/player/previous', { method: 'POST' }).then(() => undefined);
}

export function getVolume(): Promise<Volume> {
	return request<Volume>('/api/volume');
}

export function setVolume(volume: number): Promise<void> {
	return request('/api/volume', {
		method: 'POST',
		headers: { 'Content-Type': 'application/json' },
		body: JSON.stringify({ volume })
	}).then(() => undefined);
}
