<script lang="ts">
	import type { Playlist, ExistingMapping } from '$lib/types';
	import { playlistDisplayName } from '$lib/playlists';

	let {
		lastCardId,
		cardJustDetected,
		existingMapping,
		editMode = $bindable(false),
		selectedPlaylistUri = $bindable(''),
		playlists,
		saving,
		onsave,
		oncancel,
		onstartedit
	}: {
		lastCardId: string | null;
		cardJustDetected: boolean;
		existingMapping: ExistingMapping | null;
		editMode: boolean;
		selectedPlaylistUri: string;
		playlists: Playlist[];
		saving: boolean;
		onsave: () => void;
		oncancel: () => void;
		onstartedit: () => void;
	} = $props();
</script>

{#if lastCardId && cardJustDetected}
	{#if !existingMapping}
		<div
			class="mb-4 rounded-lg border-2 border-green-500 bg-green-500/10 p-4 animate-pulse"
		>
			<p class="text-sm text-gray-400">New card detected</p>
			<p class="font-mono text-lg font-bold text-green-400">{lastCardId}</p>
		</div>

		<div class="mb-3">
			<label class="mb-1 block text-sm text-gray-400" for="playlist-select">Assign playlist</label>
			<select
				id="playlist-select"
				class="w-full rounded-lg bg-gray-700 px-3 py-2 text-white outline-none focus:ring-2 focus:ring-green-500"
				bind:value={selectedPlaylistUri}
			>
				<option value="">— Select a playlist —</option>
				{#each playlists as pl}
					<option value={pl.uri}>{pl.name} ({pl.track_count} tracks)</option>
				{/each}
			</select>
		</div>

		<button
			class="rounded-lg bg-green-500 px-4 py-2 font-semibold text-white hover:bg-green-600 disabled:opacity-50 disabled:cursor-not-allowed"
			onclick={onsave}
			disabled={!selectedPlaylistUri || saving}
		>
			{saving ? 'Saving…' : 'Save Mapping'}
		</button>
	{:else}
		<div class="mb-4 rounded-lg border-2 border-blue-500 bg-blue-500/10 p-4">
			<p class="text-sm text-gray-400">Card detected</p>
			<p class="font-mono text-lg font-bold text-blue-400">{lastCardId}</p>
			<div class="mt-2 flex items-center gap-2">
				<svg class="h-4 w-4 text-blue-400" viewBox="0 0 24 24" fill="currentColor" role="img">
					<title>Mapped</title>
					<path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-2 15l-5-5 1.41-1.41L10 14.17l7.59-7.59L19 8l-9 9z"/>
				</svg>
				<p class="text-sm text-gray-300">
					Currently mapped to: <span class="font-semibold text-white">{existingMapping.playlist_name ?? playlistDisplayName(existingMapping.playlist_uri, playlists)}</span>
				</p>
			</div>
		</div>

		{#if editMode}
			<div class="mb-3">
				<label class="mb-1 block text-sm text-gray-400" for="playlist-select-edit">Change playlist</label>
				<select
					id="playlist-select-edit"
					class="w-full rounded-lg bg-gray-700 px-3 py-2 text-white outline-none focus:ring-2 focus:ring-green-500"
					bind:value={selectedPlaylistUri}
				>
					<option value="">— Select a playlist —</option>
					{#each playlists as pl}
						<option value={pl.uri}>{pl.name} ({pl.track_count} tracks)</option>
					{/each}
				</select>
			</div>

			<div class="flex gap-2">
				<button
					class="rounded-lg bg-green-500 px-4 py-2 font-semibold text-white hover:bg-green-600 disabled:opacity-50 disabled:cursor-not-allowed"
					onclick={onsave}
					disabled={!selectedPlaylistUri || saving}
				>
					{saving ? 'Saving…' : 'Update Mapping'}
				</button>
				<button
					class="rounded-lg bg-gray-700 px-4 py-2 font-semibold text-gray-300 hover:bg-gray-600"
					onclick={oncancel}
				>
					Cancel
				</button>
			</div>
		{:else}
			<button
				class="rounded-lg bg-blue-600 px-4 py-2 font-semibold text-white hover:bg-blue-500"
				onclick={onstartedit}
			>
				Change Playlist
			</button>
		{/if}
	{/if}
{:else if lastCardId}
	<div class="mb-4 rounded-lg border border-gray-700 bg-gray-700/30 p-4">
		<p class="text-sm text-gray-400">Last detected card</p>
		<p class="font-mono text-lg text-gray-200">{lastCardId}</p>
	</div>
	<p class="text-sm text-gray-500">Scan a card to assign a new mapping.</p>
{:else}
	<div class="rounded-lg border-2 border-dashed border-gray-700 p-8 text-center">
		<svg class="mx-auto mb-3 h-10 w-10 text-gray-600" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" role="img">
			<title>Scan a card</title>
			<rect x="3" y="5" width="18" height="14" rx="2"/>
			<path d="M3 10h18"/>
		</svg>
		<p class="text-gray-500">Scan a card…</p>
	</div>
{/if}
