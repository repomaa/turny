export interface AuthUrlResponse {
	url: string;
}

export interface AuthStatus {
	authenticated: boolean;
}

export interface Card {
	card_id: string;
	playlist_uri: string;
	playlist_name: string;
}

export interface Playlist {
	id: string;
	uri: string;
	name: string;
	images: string[];
	owner: string;
	track_count: number;
}

export interface NowPlaying {
	track_name: string;
	artist: string;
	album: string;
	album_art: string;
	is_playing: boolean;
	progress_ms: number;
	duration_ms: number;
}

export interface PlayerState {
	is_playing: boolean;
	current_card: string | null;
	context_uri: string | null;
}

export interface LastCard {
	card_id: string;
}

export type WsEvent =
	| { type: 'RfidDetected'; card_id: string }
	| { type: 'PlaybackStarted'; card_id: string; playlist_uri: string }
	| { type: 'PlaybackPaused' }
	| { type: 'PlaybackResumed' }
	| { type: 'StateChanged'; is_playing: boolean; current_card: string | null; context_uri: string | null };
