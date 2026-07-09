import type { Playlist } from './types';

export function playlistDisplayName(uri: string, playlists: Playlist[]): string {
	const pl = playlists.find((p) => p.uri === uri);
	return pl?.name ?? uri;
}
