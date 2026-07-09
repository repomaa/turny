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
	import type { Card, WsEvent } from '$lib/types';
	import { turnyState as state } from '$lib/state.svelte';
	import AuthScreen from '$lib/components/AuthScreen.svelte';
	import Toast from '$lib/components/Toast.svelte';
	import NowPlayingDisplay from '$lib/components/NowPlaying.svelte';
	import PlayerControls from '$lib/components/PlayerControls.svelte';
	import MappingEditor from '$lib/components/MappingEditor.svelte';
	import MappingsList from '$lib/components/MappingsList.svelte';

	let ws: WebSocketManager | null = null;
	let volumeDebounce: ReturnType<typeof setTimeout> | null = null;

	function showError(msg: string) {
		state.error = msg;
		setTimeout(() => {
			if (state.error === msg) state.error = null;
		}, 5000);
	}

	async function checkAuth() {
		try {
			const status = await getAuthStatus();
			state.authenticated = status.authenticated;
		} catch {
			state.authenticated = false;
		}
		state.authChecking = false;
	}

	async function handleLogin() {
		try {
			const { url } = await getAuthUrl();
			window.location.href = url;
		} catch {
			showError('Failed to get OAuth URL');
		}
	}

	async function handleLogout() {
		try {
			await logout();
			state.authenticated = false;
			state.nowPlaying = null;
			state.cards = [];
			state.playlists = [];
			state.lastCardId = null;
			state.existingMapping = null;
			state.editMode = false;
			state.cardJustDetected = false;
		} catch {
			showError('Failed to logout');
		}
	}

	async function refreshNowPlaying() {
		try {
			state.nowPlaying = await getNowPlaying();
		} catch {
			showError('Failed to fetch now playing');
		}
	}

	async function loadVolume() {
		try {
			const res = await getVolume();
			state.volume = res.volume;
		} catch {
			showError('Failed to load volume');
		}
	}

	async function handleVolumeChange(v: number) {
		state.volume = v;
		if (volumeDebounce) clearTimeout(volumeDebounce);
		volumeDebounce = setTimeout(async () => {
			state.volumeSending = true;
			try {
				await setVolume(v);
			} catch {
				showError('Failed to set volume');
			} finally {
				state.volumeSending = false;
			}
		}, 300);
	}

	async function loadCards() {
		try {
			state.cards = await getCards();
		} catch {
			showError('Failed to load card mappings');
		}
	}

	async function loadPlaylists() {
		try {
			state.playlists = await getPlaylists();
		} catch {
			showError('Failed to load playlists');
		}
	}

	async function handleSave() {
		if (!state.lastCardId || !state.selectedPlaylistUri) return;
		state.saving = true;
		try {
			const pl = state.playlists.find((p) => p.uri === state.selectedPlaylistUri);
			await saveCard(state.lastCardId, state.selectedPlaylistUri, pl?.name);
			await loadCards();
			state.existingMapping = {
				playlist_uri: state.selectedPlaylistUri,
				playlist_name: pl?.name ?? null
			};
			state.editMode = false;
			state.selectedPlaylistUri = '';
		} catch {
			showError('Failed to save card mapping');
		} finally {
			state.saving = false;
		}
	}

	function startEdit() {
		state.editMode = true;
		state.selectedPlaylistUri = state.existingMapping?.playlist_uri ?? '';
	}

	function cancelEdit() {
		state.editMode = false;
		state.selectedPlaylistUri = '';
	}

	async function handleDelete(id: string) {
		try {
			await deleteCard(id);
			await loadCards();
		} catch {
			showError('Failed to delete card');
		}
	}

	async function control(fn: () => Promise<void>) {
		state.playerBusy = true;
		try {
			await fn();
			await refreshNowPlaying();
		} catch {
			showError('Player command failed');
		} finally {
			state.playerBusy = false;
		}
	}

	function startEditFromList(card: Card) {
		state.lastCardId = card.card_id;
		state.existingMapping = {
			playlist_uri: card.playlist_uri,
			playlist_name: card.playlist_name
		};
		state.cardJustDetected = true;
		state.editMode = false;
		state.selectedPlaylistUri = '';
	}

	function handleWsEvent(ev: WsEvent) {
		switch (ev.type) {
			case 'RfidDetected':
				if (state.lastCardId !== ev.card_id) {
					state.lastCardId = ev.card_id;
					state.existingMapping = ev.existing_mapping;
					state.cardJustDetected = true;
					state.editMode = false;
					state.selectedPlaylistUri = '';
				}
				break;
			case 'PlaybackStarted':
			case 'PlaybackResumed':
				if (state.nowPlaying) state.nowPlaying = { ...state.nowPlaying, is_playing: true };
				refreshNowPlaying();
				break;
			case 'PlaybackPaused':
				if (state.nowPlaying) state.nowPlaying = { ...state.nowPlaying, is_playing: false };
				refreshNowPlaying();
				break;
			case 'StateChanged':
				refreshNowPlaying();
				break;
			case 'VolumeChanged':
				state.volume = ev.volume;
				break;
			case 'LagDetected':
				// WebSocket lagged — refresh all state from REST API
				refreshNowPlaying();
				loadCards();
				break;
			default:
				console.warn('Unknown WS event:', ev);
		}
	}

	const progressPct = $derived(
		state.nowPlaying && state.nowPlaying.duration_ms > 0
			? Math.max(0, Math.min(100, (state.nowPlaying.progress_ms / state.nowPlaying.duration_ms) * 100))
			: 0
	);

	const isPlaying = $derived(state.nowPlaying?.is_playing ?? false);

	onMount(() => {
		checkAuth().then(() => {
			if (state.authenticated) {
				loadPlaylists().then(() => loadCards());
				refreshNowPlaying();
				loadVolume();
			}
		});

		ws = new WebSocketManager();
		ws.on(handleWsEvent);
		ws.connect();

		return () => {
			ws?.disconnect();
		};
	});

	$effect(() => {
		if (!state.authenticated) return;
		const id = setInterval(() => {
			if (document.visibilityState === 'visible') {
				refreshNowPlaying();
			}
		}, 3000);
		return () => clearInterval(id);
	});
</script>

<div class="min-h-screen bg-gray-900 text-gray-100">
	<div class="mx-auto max-w-3xl px-4 py-8">
		<header class="mb-8 flex items-center justify-between">
			<h1 class="text-3xl font-bold text-green-500">Turny</h1>
			<AuthScreen
				authenticated={state.authenticated}
				authChecking={state.authChecking}
				onlogin={handleLogin}
				onlogout={handleLogout}
			/>
		</header>

		<Toast message={state.error} ondismiss={() => (state.error = null)} />

		{#if state.authenticated}
			<section class="mb-6 rounded-lg bg-gray-800 p-6">
				<h2 class="mb-4 text-lg font-semibold text-gray-300">Now Playing</h2>
				{#if state.nowPlaying}
					<NowPlayingDisplay nowPlaying={state.nowPlaying} {progressPct} />
					<PlayerControls
						playerBusy={state.playerBusy}
						isPlaying={isPlaying}
						onplay={() => control(playerPlay)}
						onpause={() => control(playerPause)}
						onnext={() => control(playerNext)}
						onprevious={() => control(playerPrevious)}
					/>
				{:else}
					<p class="text-gray-500">Nothing playing right now.</p>
				{/if}

				{#if state.volume !== null}
					<div class="mt-5 flex items-center gap-3">
						<svg class="h-5 w-5 shrink-0 text-gray-400" viewBox="0 0 24 24" fill="currentColor" role="img">
							<title>Volume</title>
							<path d="M3 9v6h4l5 5V4L7 9H3zm13.5 3a4.5 4.5 0 0 0-2.5-4.03v8.06A4.5 4.5 0 0 0 16.5 12z"/>
						</svg>
						<input
							type="range"
							min="0"
							max="100"
							value={state.volume}
							class="h-2 flex-1 cursor-pointer appearance-none rounded-full bg-gray-700 accent-green-500"
							aria-label="Volume"
							aria-valuetext={`${state.volume}%`}
							oninput={(e) => handleVolumeChange(Number(e.currentTarget.value))}
						/>
						<span class="w-10 shrink-0 text-right text-sm tabular-nums text-gray-400">{state.volume}%</span>
					</div>
				{/if}
			</section>

			<section class="mb-6 rounded-lg bg-gray-800 p-6">
				<h2 class="mb-4 text-lg font-semibold text-gray-300">RFID Card Mapping</h2>
				<MappingEditor
					lastCardId={state.lastCardId}
					cardJustDetected={state.cardJustDetected}
					existingMapping={state.existingMapping}
					bind:editMode={state.editMode}
					bind:selectedPlaylistUri={state.selectedPlaylistUri}
					playlists={state.playlists}
					saving={state.saving}
					onsave={handleSave}
					oncancel={cancelEdit}
					onstartedit={startEdit}
				/>
				<MappingsList
					cards={state.cards}
					playlists={state.playlists}
					onedit={startEditFromList}
					ondelete={handleDelete}
				/>
			</section>
		{:else if !state.authChecking}
			<div class="rounded-lg bg-gray-800 p-8 text-center">
				<p class="text-gray-400">Login with Spotify to start using Turny.</p>
			</div>
		{/if}

		<footer class="mt-8 text-center text-xs text-gray-600">
			Turny — Spotify RFID Controller
		</footer>
	</div>
</div>
