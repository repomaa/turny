<script lang="ts">
	import { onMount } from 'svelte';
	import { WebSocketManager } from '$lib/websocket';
	import {
		getAuthUrl,
		getAuthStatus,
		logout,
		getCards,
		saveCard,
		deleteCard,
		getPlaylists,
		getNowPlaying,
		playerPlay,
		playerPause,
		playerNext,
		playerPrevious,
		getVolume,
		setVolume
	} from '$lib/api';
	import type { Card, Playlist, NowPlaying, WsEvent, ExistingMapping } from '$lib/types';

	let authenticated = $state(false);
	let authChecking = $state(true);
	let nowPlaying = $state<NowPlaying | null>(null);
	let cards = $state<Card[]>([]);
	let playlists = $state<Playlist[]>([]);
	let lastCardId = $state<string | null>(null);
	let cardJustDetected = $state(false);
	let existingMapping = $state<ExistingMapping | null>(null);
	let editMode = $state(false);
	let selectedPlaylistUri = $state('');
	let playerBusy = $state(false);
	let saving = $state(false);
	let error = $state<string | null>(null);
	let volume = $state<number | null>(null);
	let volumeSending = $state(false);

	let ws: WebSocketManager | null = null;

	async function checkAuth() {
		try {
			const status = await getAuthStatus();
			authenticated = status.authenticated;
		} catch {
			authenticated = false;
		}
		authChecking = false;
	}

	async function handleLogin() {
		try {
			const { url } = await getAuthUrl();
			window.location.href = url;
		} catch (e) {
			error = 'Failed to get OAuth URL';
		}
	}

	async function handleLogout() {
		try {
			await logout();
			authenticated = false;
			nowPlaying = null;
			cards = [];
			playlists = [];
			lastCardId = null;
			existingMapping = null;
			editMode = false;
			cardJustDetected = false;
		} catch (e) {
			error = 'Failed to logout';
		}
	}

	async function refreshNowPlaying() {
		try {
			nowPlaying = await getNowPlaying();
		} catch {
			// ignore polling errors
		}
	}

	async function loadVolume() {
		try {
			const res = await getVolume();
			volume = res.volume;
		} catch {
			// ignore
		}
	}

	let volumeDebounce: ReturnType<typeof setTimeout> | null = null;

	async function handleVolumeChange(v: number) {
		volume = v;
		if (volumeDebounce) clearTimeout(volumeDebounce);
		volumeSending = true;
		volumeDebounce = setTimeout(async () => {
			try {
				await setVolume(v);
			} catch {
				// ignore
			}
			volumeSending = false;
		}, 300);
	}

	async function loadCards() {
		try {
			cards = await getCards();
		} catch {
			// ignore
		}
	}

	async function loadPlaylists() {
		try {
			playlists = await getPlaylists();
		} catch {
			// ignore
		}
	}

	async function handleSave() {
		if (!lastCardId || !selectedPlaylistUri) return;
		saving = true;
		try {
			const pl = playlists.find((p) => p.uri === selectedPlaylistUri);
			await saveCard(lastCardId, selectedPlaylistUri, pl?.name);
			await loadCards();
			existingMapping = {
				playlist_uri: selectedPlaylistUri,
				playlist_name: pl?.name ?? null
			};
			editMode = false;
			selectedPlaylistUri = '';
		} catch (e) {
			error = 'Failed to save card mapping';
		}
		saving = false;
	}

	function startEdit() {
		editMode = true;
		selectedPlaylistUri = existingMapping?.playlist_uri ?? '';
	}

	function cancelEdit() {
		editMode = false;
		selectedPlaylistUri = '';
	}

	function playlistDisplayName(uri: string): string {
		const pl = playlists.find((p) => p.uri === uri);
		return pl?.name ?? uri;
	}

	async function handleDelete(id: string) {
		try {
			await deleteCard(id);
			await loadCards();
		} catch (e) {
			error = 'Failed to delete card';
		}
	}

	async function control(fn: () => Promise<void>) {
		playerBusy = true;
		try {
			await fn();
			await refreshNowPlaying();
		} catch {
			// ignore
		}
		playerBusy = false;
	}

	function startEditFromList(card: Card) {
		lastCardId = card.card_id;
		existingMapping = {
			playlist_uri: card.playlist_uri,
			playlist_name: card.playlist_name
		};
		cardJustDetected = true;
		editMode = false;
		selectedPlaylistUri = '';
	}

	function handleWsEvent(ev: WsEvent) {
		switch (ev.type) {
			case 'RfidDetected':
				lastCardId = ev.card_id;
				existingMapping = ev.existing_mapping;
				cardJustDetected = true;
				editMode = false;
				selectedPlaylistUri = '';
				break;
			case 'PlaybackStarted':
			case 'PlaybackResumed':
				if (nowPlaying) nowPlaying.is_playing = true;
				refreshNowPlaying();
				break;
			case 'PlaybackPaused':
				if (nowPlaying) nowPlaying.is_playing = false;
				refreshNowPlaying();
				break;
			case 'StateChanged':
				refreshNowPlaying();
				break;
			case 'VolumeChanged':
				volume = ev.volume;
				break;
		}
	}

	const progressPct = $derived(
		nowPlaying && nowPlaying.duration_ms > 0
			? Math.min(100, (nowPlaying.progress_ms / nowPlaying.duration_ms) * 100)
			: 0
	);

	function fmt(ms: number): string {
		const s = Math.floor(ms / 1000);
		const m = Math.floor(s / 60);
		const r = s % 60;
		return `${m}:${r.toString().padStart(2, '0')}`;
	}

	onMount(() => {
		checkAuth().then(() => {
			if (authenticated) {
				loadPlaylists().then(() => loadCards());
				refreshNowPlaying();
				loadVolume();
			}
		});

		ws = new WebSocketManager();
		ws.on(handleWsEvent);
		ws.connect();

		let pollId: ReturnType<typeof setInterval> | null = null;
		// Start/stop polling based on auth state
		const pollCheck = setInterval(() => {
			if (authenticated && !pollId) {
				pollId = setInterval(refreshNowPlaying, 3000);
			} else if (!authenticated && pollId) {
				clearInterval(pollId);
				pollId = null;
			}
		}, 1000);

		return () => {
			if (pollId) clearInterval(pollId);
			clearInterval(pollCheck);
			ws?.disconnect();
		};
	});

	// Reload data when authentication changes
	$effect(() => {
		if (authenticated) {
			loadPlaylists().then(() => loadCards());
			loadVolume();
		}
	});
</script>

<div class="min-h-screen bg-gray-900 text-gray-100">
	<div class="mx-auto max-w-3xl px-4 py-8">
		<!-- Header -->
		<header class="mb-8 flex items-center justify-between">
			<h1 class="text-3xl font-bold text-green-500">Turny</h1>
			{#if authChecking}
				<span class="text-gray-400 text-sm">Checking…</span>
			{:else if authenticated}
				<div class="flex items-center gap-3">
					<span class="inline-flex items-center gap-2 rounded-lg bg-green-500/20 px-3 py-1 text-sm font-medium text-green-400">
						<svg class="h-4 w-4" viewBox="0 0 24 24" fill="currentColor"><circle cx="12" cy="12" r="6"/></svg>
						Authenticated
					</span>
					<button
						class="rounded-lg bg-gray-700 px-4 py-2 text-sm font-semibold text-white hover:bg-gray-600"
						onclick={handleLogout}
					>Logout</button>
				</div>
			{:else}
				<button
					class="rounded-lg bg-green-500 px-4 py-2 font-semibold text-white hover:bg-green-600"
					onclick={handleLogin}
				>Login with Spotify</button>
			{/if}
		</header>

		{#if error}
			<div class="mb-4 rounded-lg bg-red-900/50 border border-red-700 px-4 py-3 text-sm text-red-300">
				{error}
				<button class="ml-2 underline" onclick={() => error = null}>dismiss</button>
			</div>
		{/if}

		{#if authenticated}
			<!-- Now Playing -->
			<section class="mb-6 rounded-lg bg-gray-800 p-6">
				<h2 class="mb-4 text-lg font-semibold text-gray-300">Now Playing</h2>
				{#if nowPlaying}
					<div class="flex gap-4">
						<img
							src={nowPlaying.album_art}
							alt="Album art"
							class="h-24 w-24 rounded-lg object-cover"
						/>
						<div class="flex-1 min-w-0">
							<p class="truncate text-xl font-semibold text-white">{nowPlaying.track_name}</p>
							<p class="truncate text-gray-400">{nowPlaying.artist}</p>
							<p class="truncate text-sm text-gray-500">{nowPlaying.album}</p>
							<div class="mt-3">
								<div class="h-1.5 w-full rounded-full bg-gray-700">
									<div
										class="h-1.5 rounded-full bg-green-500 transition-all"
										style="width: {progressPct}%"
									></div>
								</div>
								<div class="mt-1 flex justify-between text-xs text-gray-500">
									<span>{fmt(nowPlaying.progress_ms)}</span>
									<span>{fmt(nowPlaying.duration_ms)}</span>
								</div>
							</div>
						</div>
					</div>

					<!-- Player Controls -->
					<div class="mt-5 flex items-center justify-center gap-4">
						<button
							class="rounded-full bg-gray-700 p-3 text-white hover:bg-gray-600 disabled:opacity-50 disabled:cursor-not-allowed"
							onclick={() => control(playerPrevious)}
							disabled={playerBusy}
							aria-label="Previous"
						>
							<svg class="h-6 w-6" viewBox="0 0 24 24" fill="currentColor"><path d="M6 6h2v12H6zm3.5 6l8.5 6V6z"/></svg>
						</button>

						{#if nowPlaying.is_playing}
							<button
								class="rounded-full bg-green-500 p-4 text-white hover:bg-green-600 disabled:opacity-50 disabled:cursor-not-allowed"
								onclick={() => control(playerPause)}
								disabled={playerBusy}
								aria-label="Pause"
							>
								<svg class="h-7 w-7" viewBox="0 0 24 24" fill="currentColor"><path d="M6 5h4v14H6zm8 0h4v14h-4z"/></svg>
							</button>
						{:else}
							<button
								class="rounded-full bg-green-500 p-4 text-white hover:bg-green-600 disabled:opacity-50 disabled:cursor-not-allowed"
								onclick={() => control(playerPlay)}
								disabled={playerBusy}
								aria-label="Play"
							>
								<svg class="h-7 w-7" viewBox="0 0 24 24" fill="currentColor"><path d="M8 5v14l11-7z"/></svg>
							</button>
						{/if}

						<button
							class="rounded-full bg-gray-700 p-3 text-white hover:bg-gray-600 disabled:opacity-50 disabled:cursor-not-allowed"
							onclick={() => control(playerNext)}
							disabled={playerBusy}
							aria-label="Next"
						>
							<svg class="h-6 w-6" viewBox="0 0 24 24" fill="currentColor"><path d="M16 6h2v12h-2zM6 18l8.5-6L6 6z"/></svg>
						</button>
					</div>
				{:else}
					<p class="text-gray-500">Nothing playing right now.</p>
				{/if}

				<!-- Volume Control -->
				{#if volume !== null}
					<div class="mt-5 flex items-center gap-3">
						<svg class="h-5 w-5 shrink-0 text-gray-400" viewBox="0 0 24 24" fill="currentColor"><path d="M3 9v6h4l5 5V4L7 9H3zm13.5 3a4.5 4.5 0 0 0-2.5-4.03v8.06A4.5 4.5 0 0 0 16.5 12z"/></svg>
						<input
							type="range"
							min="0"
							max="100"
							value={volume}
							class="h-2 flex-1 cursor-pointer appearance-none rounded-full bg-gray-700 accent-green-500"
							oninput={(e) => handleVolumeChange(Number(e.currentTarget.value))}
						/>
						<span class="w-10 shrink-0 text-right text-sm tabular-nums text-gray-400">{volume}%</span>
					</div>
				{/if}
			</section>

			<!-- RFID Card Mapping -->
			<section class="mb-6 rounded-lg bg-gray-800 p-6">
				<h2 class="mb-4 text-lg font-semibold text-gray-300">RFID Card Mapping</h2>

				{#if lastCardId && cardJustDetected}
					<!-- New card: no existing mapping -->
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
							onclick={handleSave}
							disabled={!selectedPlaylistUri || saving}
						>
							{saving ? 'Saving…' : 'Save Mapping'}
						</button>
					{:else}
						<!-- Existing card: has a mapping already -->
						<div class="mb-4 rounded-lg border-2 border-blue-500 bg-blue-500/10 p-4">
							<p class="text-sm text-gray-400">Card detected</p>
							<p class="font-mono text-lg font-bold text-blue-400">{lastCardId}</p>
							<div class="mt-2 flex items-center gap-2">
								<svg class="h-4 w-4 text-blue-400" viewBox="0 0 24 24" fill="currentColor"><path d="M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm-2 15l-5-5 1.41-1.41L10 14.17l7.59-7.59L19 8l-9 9z"/></svg>
								<p class="text-sm text-gray-300">
									Currently mapped to: <span class="font-semibold text-white">{existingMapping.playlist_name ?? playlistDisplayName(existingMapping.playlist_uri)}</span>
								</p>
							</div>
						</div>

						{#if editMode}
							<div class="mb-3">
								<label class="mb-1 block text-sm text-gray-400" for="playlist-select">Change playlist</label>
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

							<div class="flex gap-2">
								<button
									class="rounded-lg bg-green-500 px-4 py-2 font-semibold text-white hover:bg-green-600 disabled:opacity-50 disabled:cursor-not-allowed"
									onclick={handleSave}
									disabled={!selectedPlaylistUri || saving}
								>
									{saving ? 'Saving…' : 'Update Mapping'}
								</button>
								<button
									class="rounded-lg bg-gray-700 px-4 py-2 font-semibold text-gray-300 hover:bg-gray-600"
									onclick={cancelEdit}
								>
									Cancel
								</button>
							</div>
						{:else}
							<button
								class="rounded-lg bg-blue-600 px-4 py-2 font-semibold text-white hover:bg-blue-500"
								onclick={startEdit}
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
						<svg class="mx-auto mb-3 h-10 w-10 text-gray-600" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5">
							<rect x="3" y="5" width="18" height="14" rx="2"/>
							<path d="M3 10h18"/>
						</svg>
						<p class="text-gray-500">Scan a card…</p>
					</div>
				{/if}

				<!-- Existing mappings -->
				{#if cards.length > 0}
					<div class="mt-6">
						<h3 class="mb-2 text-sm font-medium text-gray-400">Existing mappings</h3>
						<ul class="divide-y divide-gray-700 rounded-lg border border-gray-700">
							{#each cards as card}
								<li class="flex items-center justify-between px-4 py-3">
									<div class="min-w-0">
										<p class="font-mono text-sm text-gray-300">{card.card_id}</p>
										<p class="truncate text-sm text-gray-500">{card.playlist_name ?? playlistDisplayName(card.playlist_uri)}</p>
									</div>
									<div class="flex items-center gap-2">
										<button
											class="rounded-lg bg-gray-700 px-3 py-1 text-sm text-gray-300 hover:bg-gray-600"
											onclick={() => startEditFromList(card)}
										>Edit</button>
										<button
											class="rounded-lg bg-red-900/50 px-3 py-1 text-sm text-red-300 hover:bg-red-900"
											onclick={() => handleDelete(card.card_id)}
										>Delete</button>
									</div>
								</li>
							{/each}
						</ul>
					</div>
				{/if}
			</section>
		{:else if !authChecking}
			<div class="rounded-lg bg-gray-800 p-8 text-center">
				<p class="text-gray-400">Login with Spotify to start using Turny.</p>
			</div>
		{/if}

		<footer class="mt-8 text-center text-xs text-gray-600">
			Turny — Spotify RFID Controller
		</footer>
	</div>
</div>
