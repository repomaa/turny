<script lang="ts">
	let {
		authenticated,
		authChecking,
		onlogin,
		onlogout
	}: {
		authenticated: boolean;
		authChecking: boolean;
		onlogin: () => void;
		onlogout: () => void;
	} = $props();
</script>

{#if authChecking}
	<span class="text-gray-400 text-sm">Checking…</span>
{:else if authenticated}
	<div class="flex items-center gap-3">
		<span class="inline-flex items-center gap-2 rounded-lg bg-green-500/20 px-3 py-1 text-sm font-medium text-green-400">
			<svg class="h-4 w-4" viewBox="0 0 24 24" fill="currentColor" role="img">
				<title>Authenticated</title>
				<circle cx="12" cy="12" r="6"/>
			</svg>
			Authenticated
		</span>
		<button
			class="rounded-lg bg-gray-700 px-4 py-2 text-sm font-semibold text-white hover:bg-gray-600"
			onclick={onlogout}
		>Logout</button>
	</div>
{:else}
	<button
		class="rounded-lg bg-green-500 px-4 py-2 font-semibold text-white hover:bg-green-600"
		onclick={onlogin}
	>Login with Spotify</button>
{/if}
