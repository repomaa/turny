<script lang="ts">
	import type { Card, Playlist } from '$lib/types';
	import { playlistDisplayName } from '$lib/playlists';

	let {
		cards,
		playlists,
		onedit,
		ondelete
	}: {
		cards: Card[];
		playlists: Playlist[];
		onedit: (card: Card) => void;
		ondelete: (id: string) => void;
	} = $props();
</script>

{#if cards.length > 0}
	<div class="mt-6">
		<h3 class="mb-2 text-sm font-medium text-gray-400">Existing mappings</h3>
		<ul class="divide-y divide-gray-700 rounded-lg border border-gray-700">
			{#each cards as card}
				<li class="flex items-center justify-between px-4 py-3">
					<div class="min-w-0">
						<p class="font-mono text-sm text-gray-300">{card.card_id}</p>
						<p class="truncate text-sm text-gray-500">{card.playlist_name ?? playlistDisplayName(card.playlist_uri, playlists)}</p>
					</div>
					<div class="flex items-center gap-2">
						<button
							class="rounded-lg bg-gray-700 px-3 py-1 text-sm text-gray-300 hover:bg-gray-600"
							onclick={() => onedit(card)}
						>Edit</button>
						<button
							class="rounded-lg bg-red-900/50 px-3 py-1 text-sm text-red-300 hover:bg-red-900"
							onclick={() => ondelete(card.card_id)}
						>Delete</button>
					</div>
				</li>
			{/each}
		</ul>
	</div>
{/if}
