import type { NowPlaying, Card, Playlist, ExistingMapping } from './types';

class TurnyState {
	authenticated = $state(false);
	authChecking = $state(true);
	nowPlaying = $state<NowPlaying | null>(null);
	cards = $state<Card[]>([]);
	playlists = $state<Playlist[]>([]);
	lastCardId = $state<string | null>(null);
	cardJustDetected = $state(false);
	existingMapping = $state<ExistingMapping | null>(null);
	editMode = $state(false);
	selectedPlaylistUri = $state('');
	playerBusy = $state(false);
	saving = $state(false);
	error = $state<string | null>(null);
	volume = $state<number | null>(null);
	volumeSending = $state(false);
}

export const turnyState = new TurnyState();
