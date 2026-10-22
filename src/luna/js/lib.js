// @luna/lib on top of Tidalfast: PlayState, MediaItem, redux
// intercept/actions, ContextMenu, StyleTag, observe and friends. Track,
// album, artist and playlist data come from the host as TIDAL API JSON.
"use strict";
(() => {
	const g = globalThis;
	const L = g.__lunaInternal;
	const { callOp, callOpAsync } = L;
	const core = L.core;
	const { Tracer, reduxStore, interceptors, buildActions, Signal } = core;

	const unloads = new Set();
	const { trace: libTrace, errSignal } = Tracer("[@luna/lib]");

	// ---------------------------------------------------------------- helpers
	const memoize = (fn) => {
		const cache = new Map();
		return (...args) => {
			const key = JSON.stringify(args);
			if (cache.has(key)) return cache.get(key);
			const v = fn(...args);
			cache.set(key, v);
			if (v instanceof Promise) v.catch(() => cache.delete(key));
			return v;
		};
	};
	const registerEmitter = (setup) => {
		const listeners = new Set();
		let started = false;
		return (unloadsSet, cb) => {
			if (!started) {
				started = true;
				setup(async (value, onErr) => {
					for (const l of [...listeners]) {
						try {
							await l(value);
						} catch (err) {
							onErr ? onErr(err) : L.reportError(err);
						}
					}
				});
			}
			listeners.add(cb);
			const unload = () => listeners.delete(cb);
			unload.__lunaPlugin = L.pluginFromStack();
			unloadsSet?.add(unload);
			return unload;
		};
	};
	const asyncDebounce = (fn) => {
		let running = null;
		let queued = null;
		return (...args) => {
			if (running) {
				queued = args;
				return running;
			}
			running = Promise.resolve(fn(...args)).finally(() => {
				running = null;
				if (queued) {
					const a = queued;
					queued = null;
					run(...a);
				}
			});
			return running;
		};
		function run(...a) {
			return fn(...a);
		}
	};
	const parseDate = (d) => {
		if (!d) return undefined;
		const date = new Date(d);
		return isNaN(date.getTime()) ? undefined : date;
	};
	const safeTimeout = (u, cb, delay) => {
		const t = setTimeout(cb, delay);
		const unload = () => clearTimeout(t);
		u?.add(unload);
		return unload;
	};
	const safeInterval = (u, cb, delay) => {
		const t = setInterval(cb, delay);
		const unload = () => clearInterval(t);
		u?.add(unload);
		return unload;
	};
	// As Luna's observable.ts: one MutationObserver (child lists,
	// subtree); for each parent the host tree mutated, the callbacks run for
	// it when it matches and for every match inside it, so an element that
	// already matched is reported again when its children change
	// (betterFullscreen finds `.is-fullscreen.is-now-playing` this way).
	// (Created here, outside any plugin, so no plugin's unload disconnects it.)
	const observed = new Set();
	let observing = false;
	const domObserver = new MutationObserver((records) => {
		if (observed.size === 0) return;
		const seen = new Set();
		const visit = (el) => {
			if (!(el instanceof Element) || seen.has(el) || !el.isConnected) return;
			seen.add(el);
			for (const entry of [...observed]) {
				const [sel, cb] = entry;
				try {
					if (el.matches(sel)) cb(el);
					for (const m of el.querySelectorAll(sel)) cb(m);
				} catch (err) {
					L.reportError(err);
				}
			}
		};
		// TIDAL's own changes report the mutated parent, as Luna does. A
		// plugin's own insertions report only what was added: our markup is
		// flatter than TIDAL's, and a callback that re-inserts next to what it
		// matched would otherwise call itself forever.
		for (const r of records) {
			if (r._host) visit(r.target);
			else for (const n of r.addedNodes) visit(n);
		}
	});
	const observe = (u, selector, cb) => {
		document.querySelector(selector);
		const entry = [selector, cb];
		observed.add(entry);
		if (!observing) {
			observing = true;
			domObserver.observe(document.documentElement, { childList: true, subtree: true });
		}
		const unload = () => observed.delete(entry);
		u?.add(unload);
		// In TIDAL, plugins load before its UI mounts, so the mount reports
		// every match. Here the UI is usually there first: what already
		// matches is reported once, on the next tick, as that mount would
		// (quickExclusiveMode's player-bar button, registered after an await).
		const present = [...document.querySelectorAll(selector)];
		if (present.length)
			setTimeout(() => {
				if (!observed.has(entry)) return;
				for (const el of present) {
					if (!el.isConnected) continue;
					try {
						cb(el);
					} catch (err) {
						L.reportError(err);
					}
				}
			}, 0);
		return unload;
	};
	// TIDAL's DOM is live; here the app's elements arrive with its next
	// drawn frame, which a background window draws less often. A wait that
	// saw no new anchor tree in its time gets more, up to five times as
	// long, so it ends on what the app shows, not on when it last drew.
	const observePromise = (u, selector, timeoutMs = 1000) =>
		new Promise((res) => {
			const found = document.querySelector(selector);
			if (found) return res(found);
			let done = false;
			const stop = observe(u, selector, (el) => {
				if (done) return;
				done = true;
				stop();
				res(el);
			});
			const syncCount = L.dom?.syncCount ?? (() => 0);
			let seen = syncCount();
			let extra = 0;
			const expire = () => {
				if (done) return;
				const now = syncCount();
				if (now - seen < 2 && extra < 4) {
					extra++;
					seen = now;
					safeTimeout(u, expire, timeoutMs);
					return;
				}
				done = true;
				stop();
				res(null);
			};
			safeTimeout(u, expire, timeoutMs);
		});

	// ---------------------------------------------------------------- redux
	const RepeatMode = { Off: 0, All: 1, One: 2, 0: "Off", 1: "All", 2: "One" };
	const interceptedLogged = {};
	function intercept(actionTypes, unloadsSet, cb, once) {
		const types = Array.isArray(actionTypes) ? actionTypes : [actionTypes];
		const plugin = L.pluginFromStack();
		const wrapped = once
			? (payload, type) => {
					unIntercept();
					return cb(payload, type);
				}
			: (payload, type) => cb(payload, type);
		wrapped.__lunaPlugin = plugin;
		const unIntercept = () => {
			for (const t of types) {
				interceptors[t]?.delete(wrapped);
				if (interceptors[t]?.size === 0) delete interceptors[t];
			}
		};
		unIntercept.source = `intercept${JSON.stringify(types)}`;
		for (const t of types) {
			(interceptors[t] ??= new Set()).add(wrapped);
			// Diagnostics: which actions plugins wait for (see redux.rs EMITTED_ACTIONS).
			if (plugin && !(interceptedLogged[plugin] ??= new Set()).has(t)) {
				interceptedLogged[plugin].add(t);
				callOp("log", { plugin, level: "debug", msg: `[luna] intercepts ${t}` });
			}
		}
		unloadsSet?.add(unIntercept);
		return unIntercept;
	}
	const interceptPromise = (type, unloadsSet, cancel) =>
		new Promise((resolve) =>
			intercept(
				type,
				unloadsSet,
				(payload) => {
					resolve(payload);
					return cancel;
				},
				true,
			),
		);
	// One call at a time per first response type, so two callers waiting for
	// the same action do not take each other's answer (Luna's Semaphore).
	const responseLocks = {};
	const obtainResponseLock = async (type) => {
		const before = responseLocks[type] ?? Promise.resolve();
		let release;
		const mine = new Promise((resolve) => (release = resolve));
		responseLocks[type] = before.then(() => mine);
		await before;
		return release;
	};
	const interceptActionResp = async (trigger, unloadsSet, resTypes, rejTypes = [], { timeoutMs = 5000, cancel } = {}) => {
		const local = new Set();
		local.add(await obtainResponseLock(resTypes[0]));
		const res = resTypes.map((t) => interceptPromise(t, local, cancel));
		// Luna resolves on these too (its doc says "rejects", its code races
		// them with the response types): the plugin reads the failure's payload.
		const rej = rejTypes.map((t) => interceptPromise(t, local, cancel));
		const to = new Promise((_, r) => safeTimeout(local, () => r(`[interceptActionResp.TIMEOUT] ${JSON.stringify([resTypes, rejTypes])}`), timeoutMs));
		if (unloadsSet) for (const u of local) unloadsSet.add(u);
		const p = Promise.race([...res, ...rej, to]).finally(() => L.unloadSet(local));
		setTimeout(trigger);
		return p;
	};
	const isActionType = (type) => typeof type === "string" && (type.includes("/") || /^[A-Z][A-Z0-9_]*$/.test(type));
	const actions = new Proxy(
		{},
		{
			// Action types are "slice/NAME" (or SCREAMING names); anything else
			// (toJSON, then, constructor, ...) is a probe, not an action.
			get: (_, type) => (isActionType(type) ? (payload) => reduxStore.dispatch(buildActions[type](payload)) : undefined),
			has: (_, type) => isActionType(type),
		},
	);
	// Every action type TIDAL's store knows (upstream's list, which it
	// exported from TIDAL with Object.keys(buildActions)); split on first read.
	let actionTypeList;
	const redux = {
		get actionTypes() {
			return (actionTypeList ??= Object.freeze(String(g.__lunaActionTypes ?? "").split(/\r?\n/).filter(Boolean)));
		},
		actions,
		intercept,
		interceptPromise,
		interceptActionResp,
		store: reduxStore,
		RepeatMode,
	};

	// `mix/LOAD_TRACK_MIX_ID` (track-radio): TIDAL's reply names the track's
	// mix, which the track resource carries.
	intercept("mix/LOAD_TRACK_MIX_ID", null, (payload) => {
		const id = payload?.id;
		(async () => {
			let mixId;
			try {
				const track = await hostGet("host.track", { id: String(id) });
				mixId = track?.mixes?.TRACK_MIX ?? track?.mixes?.trackMix;
			} catch {}
			L.internalTimeout(() => {
				if (mixId) L.hostDispatch("mix/LOAD_TRACK_MIX_ID_SUCCESS", JSON.stringify({ id, mixId }));
				else L.hostDispatch("mix/LOAD_TRACK_MIX_ID_FAIL", JSON.stringify({ id, error: "No radio available for this track" }));
			}, 0);
		})();
	});

	// ---------------------------------------------------------------- quality
	class Quality {
		constructor(idx, name, color) {
			this.idx = idx;
			this.name = name;
			this.color = color;
		}
		static HiRes = new Quality(6, "HiRes", "#ffd432");
		static MQA = new Quality(5, "MQA", "#F9BA7A");
		static Atmos = new Quality(4, "Atmos", "#6ab5ff");
		static Sony630 = new Quality(3, "Sony630", "#6ab5ff");
		static High = new Quality(2, "High", "#33FFEE");
		static Low = new Quality(1, "Low", "#b9b9b9");
		static Lowest = new Quality(0, "Lowest", "#b9b9b9");
		static Max = Quality.HiRes;
		static lookups = {
			metadataTags: { HIRES_LOSSLESS: Quality.HiRes, 6: "HIRES_LOSSLESS", MQA: Quality.MQA, 5: "MQA", DOLBY_ATMOS: Quality.Atmos, 4: "DOLBY_ATMOS", SONY_360RA: Quality.Sony630, 3: "SONY_360RA", LOSSLESS: Quality.High, 2: "LOSSLESS" },
			audioQuality: { HI_RES_LOSSLESS: Quality.HiRes, 6: "HI_RES_LOSSLESS", HI_RES: Quality.MQA, 5: "HI_RES", LOSSLESS: Quality.High, 2: "LOSSLESS", HIGH: Quality.Low, 1: "HIGH", LOW: Quality.Lowest, 0: "LOW" },
		};
		static fromIdx(i) {
			return [Quality.Lowest, Quality.Low, Quality.High, Quality.Sony630, Quality.Atmos, Quality.MQA, Quality.HiRes][i] ?? Quality.Lowest;
		}
		static fromMetaTags(tags) {
			return (tags ?? []).map((t) => Quality.lookups.metadataTags[t]).filter((q) => q instanceof Quality);
		}
		static fromAudioQuality(q) {
			return q === undefined ? undefined : Quality.lookups.audioQuality[q];
		}
		static min(...q) {
			return Quality.fromIdx(Math.min(...q.map(Number)));
		}
		static max(...q) {
			return Quality.fromIdx(Math.max(...q.map(Number)));
		}
		get audioQuality() {
			return Quality.lookups.audioQuality[this.idx];
		}
		get metadataTag() {
			return Quality.lookups.metadataTags[this.idx];
		}
		valueOf() {
			return this.idx;
		}
	}

	// ---------------------------------------------------------------- content
	const formatCoverUrl = (uuid, opts) => {
		if (!uuid) return;
		const type = opts?.type ?? "image";
		const res = opts?.res ?? "1280";
		return `https://resources.tidal.com/${type}s/${uuid.split("-").join("/")}/${res}x${res}.${type === "image" ? "jpg" : "mp4"}`;
	};
	const instances = { mediaItems: {}, albums: {}, artists: {}, playlists: {} };
	const fromStore = async (kind, id, make) => {
		if (instances[kind][id] !== undefined) return instances[kind][id];
		const fromState = reduxStore.getState().content?.[kind]?.[id];
		const inst = await make(fromState);
		if (inst === undefined) return;
		return (instances[kind][id] = inst);
	};
	const hostGet = async (op, arg) => {
		try {
			return (await callOpAsync(op, arg)) ?? undefined;
		} catch (err) {
			libTrace.warn(op, err?.message ?? err);
			return undefined;
		}
	};
	class ContentBase {
		static fromStore(id, kind, make) {
			return fromStore(kind, id, make);
		}
		static getItemFromStore(kind, id) {
			return reduxStore.getState().content?.[kind]?.[id];
		}
		static formatTitle(tidalTitle, tidalVersion, brainzTitle, brainzCredit) {
			brainzTitle = brainzTitle?.replaceAll("’", "'");
			let title = brainzTitle ?? tidalTitle;
			if (title === undefined) throw new Error("Title is undefined");
			// A title with "feat." that MusicBrainz also credits with a
			// featured artist keeps TIDAL's spelling.
			if (tidalTitle?.includes("feat. ") && !brainzTitle?.includes("feat. ")) {
				if (brainzCredit && brainzCredit.findIndex((credit) => credit.joinphrase === " feat. ") !== -1) title = tidalTitle;
			}
			if (tidalVersion && !title.toLowerCase().includes(tidalVersion.toLowerCase())) title += ` (${tidalVersion})`;
			return title;
		}
		static getAlbumCoverUrl(album, opts) {
			if (!album) return;
			let type = opts?.type ?? "image";
			if (type === "video" && !album.videoCover && opts?.fallback !== false) type = "image";
			const uuid = type === "image" ? album.cover : album.videoCover;
			if (uuid) return formatCoverUrl(uuid, opts);
			// Art from outside TIDAL's image server (local files, demo data).
			if (type === "image" && typeof album._coverUrl === "string") return album._coverUrl;
		}
		static formatCoverUrl(uuid, opts) {
			return formatCoverUrl(uuid, opts);
		}
		static async artistNames(artists) {
			const list = await artists;
			if (!list) return [];
			const out = [];
			for await (const a of list) if (a?.name) out.push(a.name);
			return out;
		}
	}
	class Artist extends ContentBase {
		static trace = libTrace.withSource(".Artist").trace;
		constructor(id, tidalArtist) {
			super();
			this.id = id;
			this.tidalArtist = tidalArtist;
		}
		static async fromId(id) {
			if (id === undefined) return;
			return fromStore("artists", id, async (a) => {
				a ??= await hostGet("host.artist", { id: String(id) });
				if (a) return new Artist(id, a);
			});
		}
		coverUrl(res) {
			return formatCoverUrl(this.tidalArtist.picture, { res });
		}
		get name() {
			return this.tidalArtist.name;
		}
	}
	const brainz = async (path) => {
		try {
			const r = await fetch(`https://musicbrainz.org/ws/2/${path}${path.includes("?") ? "&" : "?"}fmt=json`, { headers: { "User-Agent": "Tidalfast-Luna/1.0 (https://github.com/CIoudGuy/tidalfast)" } });
			if (!r.ok) return undefined;
			return await r.json();
		} catch {
			return undefined;
		}
	};
	// A song's title without what versions add to it ("(Remix)", "[Live]",
	// "feat. X", "- Remastered"), case, accents and punctuation, so that
	// TIDAL's and MusicBrainz's spellings of one song compare equal.
	const baseTitle = (title) => {
		if (typeof title !== "string") return "";
		const plain = (s) =>
			s
				.normalize("NFKD")
				.replace(/\p{M}/gu, "")
				.toLowerCase()
				.replace(/[^\p{L}\p{N}]+/gu, "");
		const base = plain(title.replace(/\s*[([].*?[)\]]/g, "").replace(/\s+(feat\.?|ft\.|featuring)\s.*$/i, "").replace(/\s+-\s.*$/, ""));
		return base || plain(title);
	};
	// Same song, by length: MusicBrainz's milliseconds against TIDAL's
	// seconds, within ten seconds (one recording's lengths differ by a few).
	const SAME_LENGTH_MS = 10000;
	const lengthGap = (tidalItem, length) => {
		const seconds = Number(tidalItem?.duration);
		if (!(seconds > 0) || !(Number(length) > 0)) return undefined;
		return Math.abs(Number(length) - seconds * 1000);
	};
	// The MusicBrainz recording for this TIDAL track among those an ISRC
	// lookup answers. MusicBrainz lets unrelated recordings share an ISRC
	// (Rihanna's "Only Girl (In the World)", USUM71023200, is also chapter
	// 57 of a Pride and Prejudice audiobook) and lists them in no fixed
	// order, so the first one can be another song. A recording counts when
	// its length is the track's or, with no length to compare, its title is;
	// the closest one wins. None counts: undefined, and the title stays
	// TIDAL's.
	const matchingRecording = (tidalItem, recordings) => {
		const title = baseTitle(tidalItem?.title);
		let best;
		let bestScore;
		for (const rec of recordings ?? []) {
			if (rec?.id === undefined) continue;
			const gap = lengthGap(tidalItem, rec.length);
			const sameTitle = title !== "" && baseTitle(rec.title) === title;
			if (gap === undefined ? !sameTitle : gap > SAME_LENGTH_MS) continue;
			const score = (sameTitle ? 0 : SAME_LENGTH_MS + 1) + (gap ?? SAME_LENGTH_MS);
			if (bestScore === undefined || score < bestScore) {
				best = rec;
				bestScore = score;
			}
		}
		return best;
	};
	class Album extends ContentBase {
		static trace = libTrace.withSource(".Album").trace;
		constructor(id, tidalAlbum) {
			super();
			this.id = id;
			this.tidalAlbum = tidalAlbum;
			this.trace = Album.trace.withSource(`[${tidalAlbum.title ?? id}]`).trace;
			this.brainzAlbum = memoize(async () => {
				const upc = this.tidalAlbum.upc;
				if (!upc) return;
				const found = (await brainz(`release/?query=barcode:${upc}`))?.releases?.[0];
				const trackCount = found?.["track-count"] ?? found?.media?.reduce((count, m) => count + (m?.["track-count"] ?? 0), 0);
				// TIDAL sometimes has the wrong UPC: only a release with
				// the album's track count is it.
				if (trackCount !== undefined && this.tidalAlbum.numberOfTracks === trackCount) return found;
				if (found !== undefined) this.trace.warn("Invalid Tidal UPC for album!", found, this);
			});
			this.brainzRelease = memoize(async () => {
				const a = await this.brainzAlbum();
				if (a?.id) return brainz(`release/${a.id}?inc=recordings+isrcs+artist-credits`);
			});
			this.artist = memoize(async () => Artist.fromId(this.tidalAlbum.artist?.id ?? this.tidalAlbum.artists?.[0]?.id));
			this.artists = memoize(() => (this.tidalAlbum.artists ?? []).map((a) => Artist.fromId(a.id)));
			this.tMediaItems = memoize(async () => (await hostGet("host.album_items", { id: String(this.id) })) ?? []);
			this.title = memoize(async () => {
				const b = await this.brainzAlbum();
				return ContentBase.formatTitle(this.tidalAlbum.title, this.tidalAlbum.version ?? undefined, b?.title, b?.["artist-credit"]);
			});
			this.upc = memoize(async () => this.tidalAlbum.upc ?? (await this.brainzAlbum())?.barcode);
		}
		static async fromId(id) {
			if (id === undefined) return;
			return fromStore("albums", id, async (a) => {
				a ??= await hostGet("host.album", { id: String(id) });
				if (a) return new Album(id, a);
			});
		}
		async count() {
			return this.tidalAlbum.numberOfTracks ?? (await this.tMediaItems()).length;
		}
		async mediaItems() {
			return MediaItem.fromTMediaItems(await this.tMediaItems());
		}
		coverUrl(opts) {
			return ContentBase.getAlbumCoverUrl(this.tidalAlbum, opts);
		}
		async brainzId() {
			return (await this.brainzAlbum())?.id;
		}
		get genre() {
			return this.tidalAlbum.genre ?? undefined;
		}
		get recordLabel() {
			return this.tidalAlbum.recordLabel ?? undefined;
		}
		get numberOfTracks() {
			return this.tidalAlbum.numberOfTracks;
		}
		get releaseDate() {
			return this.tidalAlbum.releaseDate ?? this.tidalAlbum.streamStartDate;
		}
		get releaseYear() {
			return this.releaseDate?.slice(0, 4);
		}
	}
	class Playlist extends ContentBase {
		static trace = libTrace.withSource(".Playlist").trace;
		constructor(uuid, tidalPlaylist) {
			super();
			this.uuid = uuid;
			this.tidalPlaylist = tidalPlaylist;
			this.trace = Playlist.trace.withSource(`[${tidalPlaylist.title ?? uuid}]`).trace;
			this.tMediaItems = memoize(async () => (await hostGet("host.playlist_items", { id: String(uuid) })) ?? []);
		}
		static async fromId(uuid) {
			if (uuid === undefined) return;
			return fromStore("playlists", uuid, async (p) => {
				p ??= await hostGet("host.playlist", { id: String(uuid) });
				if (p) return new Playlist(uuid, p);
			});
		}
		async count() {
			return this.tidalPlaylist.numberOfTracks ?? (await this.tMediaItems()).length;
		}
		async mediaItems() {
			return MediaItem.fromTMediaItems(await this.tMediaItems());
		}
		async title() {
			return this.tidalPlaylist.title;
		}
		coverUrl(opts) {
			return formatCoverUrl(this.tidalPlaylist.squareImage ?? this.tidalPlaylist.image, opts);
		}
	}
	class MediaItems {
		constructor(tMediaItems) {
			this.tMediaItems = tMediaItems;
		}
		static fromIds(ids, type = "track") {
			return new MediaItems(ids.map((id) => ({ item: { id }, type })));
		}
		static fromTMediaItems(t) {
			return new MediaItems(t);
		}
		async title() {
			for await (const m of await this.mediaItems()) return m.tidalItem.title;
		}
		async count() {
			return this.tMediaItems.length;
		}
		async mediaItems() {
			return MediaItem.fromTMediaItems(this.tMediaItems);
		}
	}
	let lastTransitionId;
	const qualityCache = {};
	// Luna's FLAC tags for a song (MediaItem.tags.ts); nothing is written
	// anywhere here, a plugin that tags its own files reads them.
	const makeTags = async (mediaItem) => {
		const tags = {};
		const [title, releaseDate, releaseDateStr, copyright, bpm, artistNames, brainzId, isrc, album, lyrics, coverUrl] = await Promise.all([
			mediaItem.title(),
			mediaItem.releaseDate(),
			mediaItem.releaseDateStr(),
			mediaItem.copyright(),
			mediaItem.bpm(),
			ContentBase.artistNames(mediaItem.artists()),
			mediaItem.brainzId(),
			mediaItem.isrc(),
			mediaItem.album(),
			mediaItem.lyrics(),
			mediaItem.coverUrl(),
		]);
		tags.title = title;
		tags.trackNumber = mediaItem.trackNumber?.toString();
		tags.year = releaseDate?.getFullYear().toString();
		tags.date = releaseDateStr;
		tags.REPLAYGAIN_TRACK_PEAK = mediaItem.replayGainPeak?.toString();
		tags.REPLAYGAIN_TRACK_GAIN = mediaItem.replayGain?.toString();
		tags.comment = mediaItem.url;
		tags.copyright = copyright;
		tags.bpm = bpm?.toString();
		tags.discNumber = mediaItem.volumeNumber?.toString();
		tags.artist = artistNames;
		if (tags.artist.length === 0) tags.artist = ["Unknown Artist"];
		tags.musicbrainz_trackid = brainzId;
		tags.isrc = isrc;
		if (album) {
			const [upc, brainzAlbumId, albumTitle, albumArtists] = await Promise.all([album.upc(), album.brainzId(), album.title(), ContentBase.artistNames(album.artists())]);
			tags.upc = upc;
			tags.musicbrainz_albumid = brainzAlbumId;
			tags.album = albumTitle;
			tags.albumArtist = albumArtists;
			if (tags.albumArtist.length === 0) tags.albumArtist = ["Unknown Artist"];
			tags.genres = album.genre;
			tags.organization = album.recordLabel;
			tags.totalTracks = album.numberOfTracks?.toString();
			tags.date = album.releaseDate ?? tags.date;
			tags.year = album.releaseYear ?? tags.year;
		}
		tags.lyrics = lyrics?.subtitles ?? lyrics?.lyrics;
		tags.album ??= "Unknown Album";
		return { tags, coverUrl };
	};
	class MediaItem extends ContentBase {
		static trace = libTrace.withSource(".MediaItem").trace;
		static availableTags = ["title", "trackNumber", "discNumber", "bpm", "year", "date", "copyright", "REPLAYGAIN_TRACK_GAIN", "REPLAYGAIN_TRACK_PEAK", "comment", "isrc", "upc", "musicbrainz_trackid", "musicbrainz_albumid", "artist", "album", "albumArtist", "genres", "organization", "totalTracks", "lyrics"];
		static _supportsSpatialAudio = undefined;
		static get supportsSpatialAudio() {
			return MediaItem._supportsSpatialAudio;
		}
		constructor(id, tidalMediaItem, contentType) {
			super();
			this.id = id;
			this.contentType = contentType;
			this.tidalItem = tidalMediaItem?.item ?? tidalMediaItem;
			if (this.tidalItem === undefined) throw new Error("Tidal media item is undefined!");
			if (this.tidalItem.contentType === undefined) {
				try {
					this.tidalItem.contentType = contentType;
				} catch {}
			}
			this.trace = MediaItem.trace.withSource(`[${this.tidalItem.title ?? id}]`).trace;
			this.fetchTidalMediaItem = memoize(async () => {
				const t = await hostGet("host.track", { id: String(this.id) });
				if (t) Object.assign(this.tidalItem, t);
			});
			this.brainzItem = memoize(async () => {
				// As Luna: the recording for the ISRC, then the track it
				// has on its (English, when there is one) release; failing
				// that, the album release's track at this number.
				const fromRecording = async (recording) => {
					const releases = (await brainz(`recording/${recording.id}?inc=releases+media+artist-credits+isrcs`))?.releases;
					const release = releases?.filter((r) => r["text-representation"]?.language === "eng")[0] ?? releases?.[0];
					const releaseTrack = release?.media?.[0]?.tracks?.[0];
					if (releaseTrack === undefined) return undefined;
					releaseTrack.recording ??= recording;
					return releaseTrack;
				};
				if (this.tidalItem.isrc) {
					// Not simply the first recording, as Luna takes: see
					// matchingRecording.
					const recordings = (await brainz(`isrc/${encodeURIComponent(this.tidalItem.isrc)}?inc=isrcs`))?.recordings;
					const recording = matchingRecording(this.tidalItem, recordings);
					if (recording !== undefined) return fromRecording(recording);
				}
				const albumRelease = await (await this.album())?.brainzRelease();
				if (albumRelease === undefined) return;
				const volumeNumber = (this.tidalItem.volumeNumber ?? 1) - 1;
				const trackNumber = (this.tidalItem.trackNumber ?? 1) - 1;
				const brainzItem = albumRelease.media?.[volumeNumber]?.tracks?.[trackNumber];
				if (albumRelease["text-representation"]?.language !== "eng" && brainzItem?.recording !== undefined) return (await fromRecording(brainzItem.recording)) ?? brainzItem;
				return brainzItem;
			});
			this.brainzId = memoize(async () => (await this.brainzItem())?.recording?.id);
			this.album = memoize(async () => (this.tidalItem.album?.id ? Album.fromId(this.tidalItem.album.id) : undefined));
			this.artist = memoize(async () => {
				if (this.tidalItem.artist?.id) return Artist.fromId(this.tidalItem.artist.id);
				if (this.tidalItem.artists?.[0]?.id) return Artist.fromId(this.tidalItem.artists[0].id);
				return (await this.album())?.artist();
			});
			this.artists = memoize(async () => {
				if (this.tidalItem.artists) return this.tidalItem.artists.map((a) => Artist.fromId(a.id));
				return (await this.album())?.artists() ?? [];
			});
			this.isrc = memoize(async () => {
				for await (const i of this.isrcs()) return i;
			});
			this.lyrics = memoize(async () => hostGet("host.lyrics", { id: String(this.id) }));
			this.title = memoize(async () => {
				const b = await this.brainzItem().catch(() => undefined);
				return ContentBase.formatTitle(this.tidalItem.title, this.tidalItem.version ?? undefined, b?.title, b?.["artist-credit"]);
			});
			this.releaseDate = memoize(async () => {
				let d = parseDate(this.tidalItem.releaseDate);
				if (d === undefined) d = parseDate((await this.brainzItem().catch(() => undefined))?.recording?.["first-release-date"]);
				if (d === undefined) d = parseDate((await this.album())?.releaseDate);
				return d ?? parseDate(this.tidalItem.streamStartDate);
			});
			this.releaseDateStr = memoize(async () => (await this.releaseDate())?.toISOString().slice(0, 10));
			this.coverUrl = memoize(async (opts) => ContentBase.getAlbumCoverUrl(this.tidalItem.album, opts) ?? (await this.album())?.coverUrl(opts));
			// What a spatial-only track really is, once its playback info
			// has been asked for (Luna keeps it in a persistent cache).
			Object.defineProperty(this, "cache", { value: (qualityCache[id] ??= {}), enumerable: false });
			this.fetchBestQuality = memoize(async () => {
				const allTags = Quality.fromMetaTags(this.tidalItem.mediaMetadata?.tags);
				const spatialOnly = allTags.some((q) => q === Quality.Atmos || q === Quality.Sony630) && !allTags.some((q) => q !== Quality.Atmos && q !== Quality.Sony630);
				if (spatialOnly && !this.cache.actualAudioQuality) await this.playbackInfo();
				return this.bestQuality;
			});
			// The best version of this song: the same recording (ISRC)
			// with the highest quality, or nothing when this is it.
			this.max = memoize(async () => {
				if (this.bestQuality >= Quality.Max) return;
				let best = this;
				for await (const isrc of this.isrcs()) {
					const m = await MediaItem.fromIsrc(isrc);
					if (m && m.bestQuality > best.bestQuality) {
						best = m;
						if (best.bestQuality >= Quality.Max) break;
					}
				}
				return best.id === this.id ? undefined : best;
			});
			this.playbackInfo = memoize(async (audioQuality) => {
				audioQuality ??= Quality.Max.audioQuality;
				const info = await hostGet("host.playback_info", { id: String(this.id), quality: audioQuality });
				if (!info) return undefined;
				if (info.audioQuality) this.cache.actualAudioQuality = info.audioQuality;
				rememberInfoFormat(this, audioQuality, info);
				return info;
			});
			this.flacTags = memoize(() => makeTags(this));
			this.formatListeners = new Set();
		}
		static async fromId(itemId, contentType = "track") {
			if (itemId === undefined || itemId === null) return;
			const mediaItem = await fromStore("mediaItems", itemId, async (m) => {
				m ??= await hostGet("host.track", { id: String(itemId), type: contentType });
				if (m === undefined || m === null) return;
				const item = m.item ?? m;
				L.content.mediaItems[itemId] = { item, type: contentType };
				return new MediaItem(itemId, { item }, contentType);
			});
			// A spatial-only track's real quality comes from its playback info.
			if (mediaItem && contentType === "track") await mediaItem.fetchBestQuality();
			return mediaItem;
		}
		// The best version of a recording: the track with the ISRC whose
		// quality is highest (stops at the first Max).
		static fromIsrc = memoize(async (isrc) => {
			let best;
			for await (const track of TidalApi.isrc(isrc)) {
				const maxTrackQuality = Quality.max(...Quality.fromMetaTags(track.attributes.mediaTags));
				if (maxTrackQuality > (best?.bestQuality ?? Quality.Lowest)) {
					best = (await MediaItem.fromId(track.id)) ?? best;
					if ((best?.bestQuality ?? Quality.Lowest) >= Quality.Max) return best;
				}
			}
			return best;
		});
		static async fromPlaybackContext(ctx) {
			ctx ??= PlayState.playbackContext;
			if (ctx?.actualProductId === undefined || ctx?.actualProductId === null) return undefined;
			return MediaItem.fromId(ctx.actualProductId, ctx.actualVideoQuality === null || ctx.actualVideoQuality === undefined ? "track" : "video");
		}
		static async *fromIds(ids) {
			if (!ids) return;
			for (const id of ids.filter((x) => x !== undefined)) {
				const m = await MediaItem.fromId(id);
				if (m) yield m;
			}
		}
		static async *fromTMediaItems(t) {
			if (!t) return;
			for (const x of t.filter((y) => y !== undefined)) {
				const m = await MediaItem.fromId(x.item?.id ?? x.id, x.type ?? "track");
				if (m) yield m;
			}
		}
		static onPreload = registerEmitter((emit) =>
			intercept("player/PRELOAD_ITEM", unloads, async (item) => {
				if (item?.productId === undefined) return;
				const m = await MediaItem.fromId(item.productId, item.productType);
				if (m) emit(m, m.trace.err.withContext("preloadItem.runListeners"));
			}),
		);
		static onMediaTransition = registerEmitter((emit) =>
			intercept("playbackControls/MEDIA_PRODUCT_TRANSITION", unloads, async ({ playbackContext }) => {
				const m = await MediaItem.fromPlaybackContext(playbackContext);
				if (m === undefined) return;
				// Whether the output plays spatial audio, learned from a
				// spatial track that played (Tidalfast's engine is stereo).
				if (m.tidalItem.audioModes?.includes("DOLBY_ATMOS") || m.tidalItem.audioModes?.includes("SONY_360RA")) MediaItem._supportsSpatialAudio = playbackContext?.actualAudioMode !== "STEREO";
				lastTransitionId = m.id;
				await emit(m, m.trace.err.withContext("mediaProductTransition.runListeners"));
			}),
		);
		static onPreMediaTransition = registerEmitter((emit) =>
			intercept("playbackControls/PREFILL_MEDIA_PRODUCT_TRANSITION", unloads, async ({ mediaProduct }) => {
				const m = await MediaItem.fromId(mediaProduct?.productId, mediaProduct?.productType);
				if (m) await emit(m, m.trace.err.withContext("prefillMPT.runListeners"));
			}),
		);
		play() {
			return PlayState.play(this.id);
		}
		async *isrcs() {
			if (this.contentType !== "track") return;
			const seen = new Set();
			if (this.tidalItem.isrc) {
				seen.add(this.tidalItem.isrc);
				yield this.tidalItem.isrc;
			}
			const b = await this.brainzItem().catch(() => undefined);
			for (const i of b?.recording?.isrcs ?? []) if (!seen.has(i)) {
				seen.add(i);
				yield i;
			}
		}
		async copyright() {
			if (!this.tidalItem.copyright) await this.fetchTidalMediaItem();
			return this.tidalItem.copyright ?? undefined;
		}
		async bpm() {
			if (!this.tidalItem.bpm) await this.fetchTidalMediaItem();
			return this.tidalItem.bpm ?? undefined;
		}
		get trackNumber() {
			return this.tidalItem.trackNumber;
		}
		get volumeNumber() {
			return this.tidalItem.volumeNumber;
		}
		get replayGainPeak() {
			return this.tidalItem.peak;
		}
		get replayGain() {
			return this.contentType === "track" ? this.tidalItem.replayGain : 0;
		}
		get url() {
			return this.tidalItem.url ?? `https://tidal.com/browse/${this.contentType}/${this.id}`;
		}
		get qualityTags() {
			if (this.contentType !== "track") return [];
			const tags = Quality.fromMetaTags(this.tidalItem.mediaMetadata?.tags);
			const aq = Quality.fromAudioQuality(this.tidalItem.audioQuality);
			if (tags.length === 0 && aq !== undefined && aq < Quality.High) tags.push(Quality.High);
			return tags;
		}
		get bestQuality() {
			if (this.contentType !== "track") return Quality.High;
			const allTags = Quality.fromMetaTags(this.tidalItem.mediaMetadata?.tags);
			const tags = allTags.filter((q) => q !== Quality.Atmos && q !== Quality.Sony630);
			// Spatial audio cannot say what its quality is: a spatial-only
			// track is what its playback info reported, HiRes until then.
			if (tags.length === 0 && allTags.length > 0) return Quality.fromAudioQuality(this.cache.actualAudioQuality) ?? Quality.HiRes;
			return Quality.max(...tags, Quality.fromAudioQuality(this.tidalItem.audioQuality) ?? Quality.Lowest);
		}
		get duration() {
			return this.tidalItem.duration;
		}
		async downloadProgress() {
			const plugin = L.pluginFromStack() ?? L.running ?? null;
			if (plugin === null) return undefined;
			return callOp("audio.progress", { plugin, id: String(this.id) }) ?? undefined;
		}
		async download(path, audioQuality) {
			// A plugin that downloads only to analyse the audio, whose
			// analysis Tidalfast answers from the live output (media.js),
			// gets nothing saved and no error.
			const plugin = L.pluginFromStack() ?? L.running ?? null;
			if (L.media?.liveAnalysis?.has(plugin)) return;
			// Every other download needs the user's permission (asked here
			// at first use) and goes only where the user picked for this
			// plugin (otherwise the app asks for a folder first).
			const target = Array.isArray(path) ? path.filter((p) => p !== undefined && p !== null && p !== "").map(String).join("/") : path;
			if (typeof target !== "string" || target === "") throw new Error("MediaItem.download needs a file path");
			await L.permit({ plugin, kind: "save_audio" });
			const r = await callOpAsync("audio.save", { plugin, id: String(this.id), quality: audioQuality ?? null, path: target });
			return r?.path;
		}
		async fileExtension() {
			return "flac";
		}
		// The stream's format, as Luna's MediaItem reads it from TIDAL's
		// manifest: { audioQuality, bitDepth, sampleRate, codec, bitrate,
		// duration }. Tidalfast never hands plugins a manifest, stream URL
		// or DRM data; the playing track's format comes from the local
		// engine (playbackContext), others from what their quality tier
		// fixes (16-bit 44.1 kHz FLAC for LOSSLESS, AAC 320/96 kb/s) or what
		// was seen when they played.
		withFormat(u, audioQuality, listener) {
			const entry = { audioQuality, listener };
			watchFormats();
			this.formatListeners.add(entry);
			const unload = () => this.formatListeners.delete(entry);
			u?.add(unload);
			const known = knownFormat(this.id, audioQuality);
			if (known) {
				try {
					listener(known);
				} catch (err) {
					this.trace.err.withContext("withFormat")(err);
				}
			}
			return unload;
		}
		async updateFormat(audioQuality, force) {
			if (this.contentType !== "track") return undefined;
			const id = String(this.id);
			const playing = () => {
				const ctx = PlayState.playbackContext;
				return ctx && String(ctx.actualProductId) === id && (audioQuality === undefined || audioQuality === null || ctx.actualAudioQuality === audioQuality) ? ctx : undefined;
			};
			let fmt;
			let fromEngine = false;
			if (playing()) {
				// The engine reports the decoded format a moment after the song starts.
				for (let i = 0; i < 20 && !playing()?.sampleRate; i++) await new Promise((r) => setTimeout(r, 250));
				const ctx = playing();
				if (ctx?.sampleRate) {
					fmt = engineFormat(ctx);
					fromEngine = true;
				}
			}
			const q = audioQuality ?? PlayState.playbackContext?.actualAudioQuality ?? this.bestQuality.audioQuality;
			// The engine's figures, else what TIDAL's playback info says
			// (asked here as Luna asks it: bit depth, sample rate, and the
			// bit rate from the manifest or the file's size), else what the
			// tier fixes.
			const tier = tierFormat(q, this.duration);
			if (!fmt && (force || !engineKeys.has(`${id}|${q}`))) {
				const info = await this.playbackInfo(q).catch(() => undefined);
				if (info) fmt = formatCache.get(`${id}|${info.audioQuality ?? q}`);
			}
			fmt ??= (!force && knownFormat(id, q)) || formatCache.get(`${id}|${q}`) || tier;
			if (!fmt) return undefined;
			formatCache.set(`${id}|${fmt.audioQuality}`, fmt);
			if (fromEngine) engineKeys.add(`${id}|${fmt.audioQuality}`);
			for (const { audioQuality: want, listener } of this.formatListeners) {
				if (want !== undefined && want !== null && want !== fmt.audioQuality) continue;
				try {
					await listener(fmt);
				} catch (err) {
					this.trace.err.withContext("updateFormat.listener")(err);
				}
			}
			return fmt;
		}
	}
	// Formats seen, by `id|audioQuality`.
	const formatCache = new Map();
	// The keys of formats the engine reported (they stay over a playback info's).
	const engineKeys = new Set();
	// What TIDAL's playback info says of a song (bit depth, sample rate, and
	// the codec and bit rate its manifest names; the manifest itself never
	// reaches plugins) is the song's format for `withFormat` and
	// `updateFormat`, in place of the tier's assumption. The format the
	// engine reported while the song played is kept over it.
	const rememberInfoFormat = (item, asked, info) => {
		const quality = info.audioQuality ?? asked;
		const key = `${item.id}|${quality}`;
		if (!quality || engineKeys.has(key)) return;
		const tier = tierFormat(quality, item.duration);
		const stream = info.stream ?? {};
		formatCache.set(key, {
			...tier,
			audioQuality: quality,
			bitDepth: info.bitDepth ?? tier?.bitDepth,
			sampleRate: info.sampleRate ?? tier?.sampleRate,
			codec: stream.codec ?? tier?.codec,
			bitrate: stream.bitrate ?? (stream.bytes && item.duration ? Math.round((stream.bytes * 8) / item.duration) : tier?.bitrate),
			duration: item.duration,
		});
	};
	const knownFormat = (id, q) => {
		if (q === undefined || q === null) {
			for (const [k, v] of formatCache) if (k.startsWith(`${id}|`)) return v;
			return undefined;
		}
		return formatCache.get(`${id}|${q}`);
	};
	const engineFormat = (ctx) => {
		const codec = ctx.codec ? String(ctx.codec).toLowerCase() : undefined;
		const fmt = { audioQuality: ctx.actualAudioQuality, sampleRate: ctx.sampleRate ?? undefined, bitDepth: ctx.bitDepth ?? undefined, codec, duration: ctx.actualDuration ?? undefined };
		const tier = tierFormat(ctx.actualAudioQuality);
		// The engine's bit rate (the manifest's, else the file's size over
		// its length, as TidaLuna works it out); a lossy tier fixes one.
		if (ctx.bitrate) fmt.bitrate = ctx.bitrate;
		else if (tier?.bitrate && codec !== "flac") fmt.bitrate = tier.bitrate;
		return fmt;
	};
	// What a TIDAL quality tier fixes about its stream.
	const tierFormat = (q, duration) => {
		switch (q) {
			case "LOW":
				return { audioQuality: q, sampleRate: 44100, codec: "aac", bitrate: 96000, duration };
			case "HIGH":
				return { audioQuality: q, sampleRate: 44100, codec: "aac", bitrate: 320000, duration };
			case "LOSSLESS":
				return { audioQuality: q, sampleRate: 44100, bitDepth: 16, codec: "flac", duration };
			case "HI_RES_LOSSLESS":
				return { audioQuality: q, bitDepth: 24, codec: "flac", duration };
			default:
				return undefined;
		}
	};
	// The engine's format reaches songs as they play: listeners of the
	// playing track hear it without asking (Tidal Tags' column of a row).
	let lastFormatKey = "";
	let formatWatch = null;
	const watchFormats = () => {
		formatWatch ??= reduxStore.subscribe(() => formatState());
	};
	const formatState = () => {
		const ctx = PlayState.playbackContext;
		if (!ctx?.actualProductId || !ctx.sampleRate) return;
		const key = `${ctx.actualProductId}|${ctx.actualAudioQuality}|${ctx.sampleRate}|${ctx.bitDepth}|${ctx.bitrate}`;
		if (key === lastFormatKey) return;
		lastFormatKey = key;
		formatCache.set(`${ctx.actualProductId}|${ctx.actualAudioQuality}`, engineFormat(ctx));
		engineKeys.add(`${ctx.actualProductId}|${ctx.actualAudioQuality}`);
	};

	// ---------------------------------------------------------------- PlayState
	class PlayState {
		static MIN_SCROBBLE_DURATION = 240000;
		static MIN_SCROBBLE_PERCENTAGE = 0.5;
		static cumulativePlaytime = 0;
		static lastPlayStart = undefined;
		static trace = libTrace.withSource(".PlayState").trace;
		static get playbackControls() {
			return reduxStore.getState().playbackControls;
		}
		static get playbackContext() {
			return this.playbackControls.playbackContext;
		}
		static get playQueue() {
			return reduxStore.getState().playQueue;
		}
		static get currentTime() {
			return this.playbackControls.latestCurrentTime ?? 0;
		}
		static nextMediaItem() {
			return MediaItem.fromId(this.playQueue.elements[this.playQueue.currentIndex + 1]?.mediaItemId);
		}
		static previousMediaItem() {
			return MediaItem.fromId(this.playQueue.elements[this.playQueue.currentIndex - 1]?.mediaItemId);
		}
		static get state() {
			return this.playbackControls.playbackState;
		}
		static get desiredState() {
			return this.playbackControls.desiredPlaybackState;
		}
		static get shuffle() {
			return this.playQueue.shuffleModeEnabled;
		}
		static setShuffle(shuffle, shuffleItems = false) {
			if (shuffleItems) return shuffle ? actions["playQueue/ENABLE_SHUFFLE_MODE_AND_SHUFFLE_ITEMS"]({ shuffleSeed: Math.random() }) : actions["playQueue/DISABLE_SHUFFLE_MODE_AND_UNSHUFFLE_ITEMS"]();
			if (shuffle !== this.shuffle) shuffle ? actions["playQueue/ENABLE_SHUFFLE_MODE"]() : actions["playQueue/DISABLE_SHUFFLE_MODE"]();
		}
		static RepeatMode = RepeatMode;
		static get repeatMode() {
			return this.playQueue.repeatMode;
		}
		static setRepeatMode(m) {
			actions["playQueue/SET_REPEAT_MODE"](m);
		}
		static get playing() {
			return this.desiredState === "PLAYING";
		}
		static play(mediaItemId) {
			if (mediaItemId !== undefined) {
				this.playNext(mediaItemId);
				this.next();
			}
			actions["playbackControls/PLAY"]();
		}
		static pause() {
			actions["playbackControls/PAUSE"]();
		}
		static next() {
			actions["playQueue/MOVE_NEXT"]();
		}
		static previous() {
			actions["playQueue/MOVE_PREVIOUS"]();
		}
		static moveTo(i) {
			actions["playQueue/MOVE_TO"](i);
		}
		static playNext(ids) {
			ids = Array.isArray(ids) ? ids : [ids];
			actions["playQueue/ADD_NEXT"]({ mediaItemIds: ids, context: { type: "UNKNOWN" } });
		}
		static updatePlayQueue(q) {
			actions["playQueue/RESET"]({ ...PlayState.playQueue, ...q });
		}
		static get playTime() {
			return this.playbackControls.latestCurrentTime;
		}
		static seek(t) {
			actions["playbackControls/SEEK"](t);
		}
		static onScrobble = registerEmitter(async (onScrobble) => {
			let current = await MediaItem.fromPlaybackContext();
			let done = false;
			const check = (m) => {
				if (done || m.duration === undefined) return;
				if (PlayState.lastPlayStart !== undefined) {
					PlayState.cumulativePlaytime += Date.now() - PlayState.lastPlayStart;
					PlayState.lastPlayStart = Date.now();
				}
				const enough = PlayState.cumulativePlaytime >= PlayState.MIN_SCROBBLE_DURATION || PlayState.cumulativePlaytime >= m.duration * PlayState.MIN_SCROBBLE_PERCENTAGE * 1000;
				if (enough) {
					done = true;
					onScrobble(m, PlayState.trace.err.withContext("onScrobble"));
				}
			};
			MediaItem.onMediaTransition(unloads, (m) => {
				if (current !== undefined) check(current);
				current = m;
				PlayState.cumulativePlaytime = 0;
				if (PlayState.lastPlayStart !== undefined) PlayState.lastPlayStart = Date.now();
				done = false;
			});
			intercept("playbackControls/SET_PLAYBACK_STATE", unloads, (s) => {
				if (s === "IDLE" && current !== undefined) check(current);
			});
		});
		static onState = registerEmitter((emit) => intercept("playbackControls/SET_PLAYBACK_STATE", unloads, (s) => emit(s, PlayState.trace.err.withContext("onState"))));
	}
	intercept("playbackControls/SET_PLAYBACK_STATE", unloads, (state) => {
		if (state === "PLAYING") PlayState.lastPlayStart ??= Date.now();
		else {
			if (PlayState.lastPlayStart !== undefined) PlayState.cumulativePlaytime += Date.now() - PlayState.lastPlayStart;
			PlayState.lastPlayStart = undefined;
		}
	});

	// ---------------------------------------------------------------- context menu
	// The host opens a menu for some media items; plugins add buttons and
	// call show(); the buttons shown are sent back to the host as menu entries.
	const menuButtons = new Map();
	let buttonSeq = 1;
	const fakeMenu = () => {
		const el = document.createElement("div");
		el.setAttribute("data-type", "list-container__context-menu");
		return el;
	};
	let shown = [];
	class ContextMenuButton {
		constructor(plugin) {
			this.id = buttonSeq++;
			this.plugin = plugin;
			this._text = "";
			this._onClick = undefined;
			this._element = undefined;
		}
		get elem() {
			return this._element;
		}
		// As Luna's: a new element takes the button's text and click.
		set elem(e) {
			if (this._element !== undefined && this._element !== e) this._element.parentElement?.parentElement?.remove?.();
			this._element = e;
			if (e === undefined) return;
			e.innerText = this._text;
			e.onclick = (ev) => {
				ev?.preventDefault?.();
				this._onClick?.(ev);
			};
		}
		get text() {
			return this._text;
		}
		set text(t) {
			this._text = String(t);
			if (this._element !== undefined) this._element.innerText = this._text;
			this._sync(this._text);
		}
		// What the native menu draws of the element: the text, the text colour
		// and whether the plugin hid it (`style.color`, `display: none`,
		// `visibility: hidden`, `hidden`); the rest of its style is not
		// drawn (the entries are the app's own rows).
		_look() {
			const el = this._element;
			const st = el?.style;
			const color = typeof st?.color === "string" && st.color.trim() !== "" ? st.color.trim() : null;
			const hidden = !!el && (st?.display === "none" || st?.visibility === "hidden" || el.hidden === true || el.parentElement?.style?.display === "none");
			return { color, hidden };
		}
		// What the native menu shows for this button: its text, which the plugin
		// may also have changed on the element itself.
		_sync(text) {
			const s = shown.find((x) => x.id === this.id);
			if (!s) return;
			const look = this._look();
			if (s.text === text && s.color === look.color && s.hidden === look.hidden) return;
			s.text = text;
			Object.assign(s, look);
			callOp("context_menu.items", { items: shown });
		}
		async show(contextMenu) {
			if (!contextMenu) return this.elem;
			if (this._element === undefined) {
				// The element Luna hands out: the span of a `contextmenu-item` row.
				const row = document.createElement("div");
				row.setAttribute("data-type", "contextmenu-item");
				const button = document.createElement("button");
				const span = document.createElement("span");
				button.appendChild(span);
				row.appendChild(button);
				this.elem = span;
			}
			if (!shown.some((x) => x.id === this.id)) shown.push({ id: this.id, plugin: this.plugin ?? null, text: this._text, ...this._look() });
			callOp("context_menu.items", { items: shown });
			// The plugin styles and words its element after show() returns.
			L.internalTimeout(() => {
				const t = this._element?.innerText ?? this._element?.textContent;
				if (typeof t === "string" && t !== "" && t !== this._text) this._text = t;
				this._sync(this._text);
			}, 0);
			return this._element;
		}
		onClick(cb) {
			this._onClick = cb;
		}
	}
	class ContextMenu {
		static trace = libTrace.withSource(".ContextMenu").trace;
		static buttons = new Set();
		static addButton(u) {
			const b = new ContextMenuButton(L.pluginFromStack());
			ContextMenu.buttons.add(b);
			menuButtons.set(b.id, b);
			u?.add(() => {
				ContextMenu.buttons.delete(b);
				menuButtons.delete(b.id);
			});
			return b;
		}
		static async getCurrent() {
			// The native menu is a real node of the document once it is
			// drawn (a frame or two after it opens); a detached stand-in
			// serves plugins until then.
			const real = () => document.querySelector('[data-test="contextmenu"]');
			let el = real();
			for (let i = 0; !el && ContextMenu._current && i < 8; i++) {
				await new Promise((resolve) => L.internalTimeout(resolve, 40));
				el = real();
			}
			return el ?? ContextMenu._current ?? null;
		}
		static onOpen = registerEmitter((onOpen) => {
			intercept("contextMenu/OPEN", unloads, async (event) => {
				const contextMenu = await ContextMenu.getCurrent();
				if (contextMenu) onOpen({ event, contextMenu }, ContextMenu.trace.msg.err.withContext(".onOpen"));
			});
		});
		static onMediaItem = registerEmitter((onMediaItem) => {
			intercept("contextMenu/OPEN_MEDIA_ITEM", unloads, async (item) => {
				const contextMenu = await ContextMenu.getCurrent();
				if (contextMenu) onMediaItem({ mediaCollection: MediaItems.fromIds([item.id]), contextMenu }, ContextMenu.trace.err.withContext("onMediaItem"));
			});
			intercept("contextMenu/OPEN_MULTI_MEDIA_ITEM", unloads, async (items) => {
				const contextMenu = await ContextMenu.getCurrent();
				if (contextMenu) onMediaItem({ mediaCollection: MediaItems.fromIds(items.ids), contextMenu }, ContextMenu.trace.err.withContext("onMediaItem"));
			});
			ContextMenu.onOpen(unloads, async ({ event, contextMenu }) => {
				if (event.type === "ALBUM") {
					const a = await Album.fromId(event.id);
					if (a) onMediaItem({ mediaCollection: a, contextMenu });
				} else if (event.type === "PLAYLIST") {
					const p = await Playlist.fromId(event.id);
					if (p) onMediaItem({ mediaCollection: p, contextMenu });
				}
			});
		});
	}
	// arg: { kind: "track" | "tracks" | "album" | "playlist", ids: [...] }
	L.contextMenuOpen = ({ kind, ids }) => {
		shown = [];
		ContextMenu._current = fakeMenu();
		callOp("context_menu.items", { items: shown });
		const id = ids?.[0];
		if (kind === "playlist") L.hostDispatch("contextMenu/OPEN", JSON.stringify({ type: "PLAYLIST", id, position: [0, 0] }));
		else if (kind === "album") L.hostDispatch("contextMenu/OPEN", JSON.stringify({ type: "ALBUM", id, position: [0, 0] }));
		else if (kind === "artist") L.hostDispatch("contextMenu/OPEN", JSON.stringify({ type: "ARTIST", id, position: [0, 0] }));
		else if (kind === "mix") L.hostDispatch("contextMenu/OPEN", JSON.stringify({ type: "MIX", id, position: [0, 0] }));
		else if ((ids?.length ?? 0) > 1) L.hostDispatch("contextMenu/OPEN_MULTI_MEDIA_ITEM", JSON.stringify({ type: "MULTI_MEDIA_ITEM", ids, position: [0, 0], sourceContext: { type: "UNKNOWN" } }));
		else L.hostDispatch("contextMenu/OPEN_MEDIA_ITEM", JSON.stringify({ type: "MEDIA_ITEM", id, position: [0, 0], canBlock: false, contextMenuLocation: "trackList", sourceContext: { type: "UNKNOWN" } }));
	};
	L.contextMenuClick = ({ id }) => {
		const b = menuButtons.get(id);
		if (!b) return;
		const ev = { preventDefault() {}, stopPropagation() {}, target: b._element, currentTarget: b._element };
		try {
			const r = b._onClick?.(ev);
			if (r instanceof Promise) r.catch(L.reportError);
		} catch (err) {
			L.reportError(err);
		}
		ContextMenu._current = null;
		L.hostDispatch("contextMenu/CLOSE", "null");
	};
	L.contextMenuDrop = (plugin) => {
		for (const [id, b] of menuButtons) if (b.plugin === plugin) menuButtons.delete(id);
		const before = shown.length;
		shown = shown.filter((s) => s.plugin !== plugin);
		if (shown.length !== before) callOp("context_menu.items", { items: shown });
	};

	// ---------------------------------------------------------------- StyleTag
	class StyleTag {
		constructor(id, u, css) {
			this.id = id;
			this.styleTag = (id ? document.getElementById(id) : null) ?? document.createElement("style");
			if (id) this.styleTag.id = id;
			this.css = css;
			u?.add(this.remove.bind(this));
		}
		remove() {
			this.styleTag.remove();
		}
		add() {
			document.head.appendChild(this.styleTag);
		}
		get css() {
			return this._css;
		}
		set css(css) {
			this._css = css?.trim();
			if (this._css === undefined || this._css === "") this.remove();
			else {
				this.styleTag.innerHTML = this._css;
				if (!this.styleTag.isConnected) this.add();
			}
		}
	}

	// ---------------------------------------------------------------- TidalApi / misc
	const TidalApi = {
		trace: libTrace.withSource("TidalApi").trace,
		track: (id) => hostGet("host.track", { id: String(id) }),
		album: (id) => hostGet("host.album", { id: String(id) }),
		artist: (id) => hostGet("host.artist", { id: String(id) }),
		playlist: (id) => hostGet("host.playlist", { id: String(id) }),
		albumItems: (id) => hostGet("host.album_items", { id: String(id) }),
		// As Luna: a page object with every item, not the bare list.
		playlistItems: async (id) => {
			const items = (await hostGet("host.playlist_items", { id: String(id) })) ?? [];
			return { items, totalNumberOfItems: items.length, offset: 0, limit: -1 };
		},
		lyrics: (id) => hostGet("host.lyrics", { id: String(id) }),
		playbackInfo: (id, q) => hostGet("host.playback_info", { id: String(id), quality: q ?? null }),
		// Every track with the ISRC, in Luna's JSON:API shape. The tracks
		// are also put in the store, so MediaItem.fromId does not ask again.
		async *isrc(isrc) {
			for (const t of (await hostGet("host.isrc_tracks", { isrc })) ?? []) {
				if (t?.id === undefined || t?.id === null) continue;
				L.content.mediaItems[t.id] ??= { item: t, type: "track" };
				yield {
					id: String(t.id),
					type: "tracks",
					attributes: { title: t.title, version: t.version ?? undefined, isrc: t.isrc ?? isrc, explicit: !!t.explicit, popularity: t.popularity, mediaTags: t.mediaMetadata?.tags ?? [] },
				};
			}
		},
		getCredentials: () => getCredentials(),
		// Luna's own statics. Tidalfast never hands plugins the account's
		// token: these headers name the signed-in account, and Tidalfast
		// answers GETs to TIDAL's v1 API that carry them with its own
		// session (read only; see luna_host.rs).
		getAuthHeaders: async () => ({ Authorization: `Bearer ${ACCOUNT_TOKEN}`, "x-tidal-token": ACCOUNT_TOKEN }),
		queryArgs: () => {
			const s = redux.store.getState();
			return `countryCode=${s.session?.countryCode ?? "US"}&deviceType=DESKTOP&locale=${s.settings?.language ?? "en_US"}`;
		},
		fetch: async (url) => {
			const res = await g.fetch(url, { headers: await TidalApi.getAuthHeaders() });
			if (res.status >= 200 && res.status < 300) return res.json();
			if (res.status === 403 || res.status === 404) return undefined;
			throw new Error(`${res.status} ${res.statusText}`);
		},
	};
	const ACCOUNT_TOKEN = "tidalfast-account";
	// The TIDAL sign-in, only for a plugin the user allowed to have it
	// (asked at first use): TidaLuna's shape, with the current token.
	const getCredentials = async () => {
		const plugin = L.pluginFromStack() ?? L.running ?? null;
		await L.permit({ plugin, kind: "credentials" });
		const c = await callOpAsync("host.credentials", { plugin });
		if (!c) throw new Error("You are not signed in to TIDAL");
		return c;
	};
	// Luna's `Tidal`: PlayState, and TIDAL's feature flags from the store
	// with the user's overrides applied.
	const Tidal = {
		PlayState,
		get featureFlags() {
			const { flags = {}, userOverrides = {} } = redux.store.getState().featureFlags ?? {};
			const out = {};
			for (const [k, f] of Object.entries(flags)) out[k] = { ...f };
			for (const [k, v] of Object.entries(userOverrides)) out[k] = { ...(out[k] ?? { name: k, type: "BOOLEAN", created: 0 }), value: v };
			return out;
		},
	};
	const ipcRenderer = {
		invoke: (...a) => g.__ipcRenderer.invoke(...a),
		send: (...a) => g.__ipcRenderer.send(...a),
		on: (u, ch, l) => {
			const un = g.__ipcRenderer.on(ch, l);
			u?.add(un);
			return un;
		},
		once: (u, ch, l) => {
			const un = g.__ipcRenderer.once(ch, l);
			u?.add(un);
			return un;
		},
		onOpenUrl: (u, l) => ipcRenderer.on(u, "__Luna.openUrl", l),
	};

	// `parseDasha(text, url)`: Luna's wrapper around the `dasha` parser,
	// for a DASH manifest the plugin already has (Tidalfast's
	// getPlaybackInfo never hands out manifests). The same plain-JSON shape:
	// { duration, tracks: { all, videos, audios, subtitles } }, each track
	// with id, type, codec, bitrate { bps, kbps, mbps, gbps } and segments.
	const parseDasha = async (text, manifestUrl = "", fallbackLanguage) => {
		text = String(text ?? "");
		if (!text.includes("<MPD")) throw new Error(text.includes("#EXTM3U") ? "HLS manifests are not parsed in Tidalfast's Luna" : "Invalid manifest");
		const root = { tag: "#root", attrs: {}, kids: [] };
		const stack = [root];
		const decode = (s) => s.replace(/&(amp|lt|gt|quot|apos);/g, (_, e) => ({ amp: "&", lt: "<", gt: ">", quot: '"', apos: "'" })[e]);
		const tagRe = /<(\/?)([\w:.-]+)((?:\s+[\w:.-]+\s*=\s*(?:"[^"]*"|'[^']*'))*)\s*(\/?)>|<!--[\s\S]*?-->|<\?[\s\S]*?\?>|<!\[CDATA\[([\s\S]*?)\]\]>|([^<]+)/g;
		for (let m; (m = tagRe.exec(text)); ) {
			const top = stack[stack.length - 1];
			if (m[6] !== undefined || m[5] !== undefined) {
				const t = m[5] ?? decode(m[6]).trim();
				if (t) top.text = (top.text ?? "") + t;
			} else if (m[2]) {
				if (m[1]) {
					if (stack.length > 1) stack.pop();
					continue;
				}
				const el = { tag: m[2].replace(/^.*:/, ""), attrs: {}, kids: [] };
				for (const a of m[3].matchAll(/([\w:.-]+)\s*=\s*(?:"([^"]*)"|'([^']*)')/g)) el.attrs[a[1]] = decode(a[2] ?? a[3]);
				top.kids.push(el);
				if (!m[4]) stack.push(el);
			}
		}
		const kid = (el, tag) => el?.kids.find((k) => k.tag === tag);
		const kids = (el, tag) => el?.kids.filter((k) => k.tag === tag) ?? [];
		const mpd = kid(root, "MPD");
		if (!mpd) throw new Error("Invalid manifest");
		const seconds = (s) => {
			const m = /P(?:(\d*)Y)?(?:(\d*)M)?(?:(\d*)D)?(?:T(?:(\d*)H)?(?:(\d*)M)?(?:([\d.]*)S)?)?/.exec(s ?? "");
			if (!m) return 0;
			const [y, mo, d, h, mi, se] = m.slice(1).map((v) => Number(v || 0));
			return y * 31536000 + mo * 2592000 + d * 86400 + h * 3600 + mi * 60 + se;
		};
		const join = (base, rel) => {
			if (!rel) return base;
			try {
				return new URL(rel, base || undefined).toString();
			} catch {
				return rel;
			}
		};
		const bitrate = (bps) => ({ bps, kbps: Math.round(bps / 1000), mbps: Math.round(bps / 1e4) / 100, gbps: Math.round(bps / 1e7) / 100 });
		const audioCodec = (c) => (/^mp4a/.test(c) ? "AAC" : /^flac/i.test(c) ? "FLAC" : /^ec-3/.test(c) ? "DD+" : /^ac-3/.test(c) ? "DD" : /^opus/i.test(c) ? "OPUS" : /^alac/.test(c) ? "ALAC" : /^vorbis/.test(c) ? "VORB" : /^dts/.test(c) ? "DTS" : undefined);
		const videoCodec = (c) => (/^(avc|h264)/.test(c) ? "H.264" : /^(hvc|hev)/.test(c) ? "H.265" : /^vp0?9/.test(c) ? "VP9" : /^vp0?8/.test(c) ? "VP8" : /^av01/.test(c) ? "AV1" : undefined);
		const fill = (tpl, f) =>
			String(tpl).replace(/\$(\w*)(?:%0(\d+)d)?\$/g, (all, k, w) => {
				if (all === "$$") return "$";
				if (f[k] === undefined) return all;
				const v = String(f[k]);
				return k === "RepresentationID" || !w ? v : v.padStart(Number(w), "0");
			});
		const duration = seconds(mpd.attrs.mediaPresentationDuration) || undefined;
		const all = [];
		const mpdBase = join(manifestUrl, kid(mpd, "BaseURL")?.text);
		for (const period of kids(mpd, "Period")) {
			const periodBase = join(mpdBase, kid(period, "BaseURL")?.text);
			const periodDuration = seconds(period.attrs.duration) || duration;
			for (const set of kids(period, "AdaptationSet")) {
				for (const rep of kids(set, "Representation")) {
					const get = (k) => rep.attrs[k] ?? set.attrs[k];
					const base = join(join(periodBase, kid(set, "BaseURL")?.text), kid(rep, "BaseURL")?.text);
					const codecs = get("codecs") ?? "";
					const mime = get("mimeType") ?? "";
					const type = audioCodec(codecs) ? "audio" : videoCodec(codecs) ? "video" : (get("contentType") ?? mime.split("/")[0]) === "text" ? "text" : (get("contentType") ?? mime.split("/")[0]);
					const fields = { RepresentationID: get("id"), Bandwidth: get("bandwidth") };
					const segments = [];
					const tpl = kid(rep, "SegmentTemplate") ?? kid(set, "SegmentTemplate");
					const list = kid(rep, "SegmentList") ?? kid(set, "SegmentList");
					if (tpl) {
						const start = Number(tpl.attrs.startNumber ?? 1);
						const media = join(base, tpl.attrs.media);
						const timeline = kid(tpl, "SegmentTimeline");
						if (timeline) {
							let t = 0, n = start;
							for (const s of kids(timeline, "S")) {
								if (s.attrs.t) t = Number(s.attrs.t);
								for (let r = 0; r <= Number(s.attrs.r ?? 0); r++, n++, t += Number(s.attrs.d)) segments.push({ url: fill(media, { ...fields, Number: n, Time: t }) });
							}
						} else if (tpl.attrs.duration) {
							const count = periodDuration ? Math.ceil(periodDuration / (Number(tpl.attrs.duration) / Number(tpl.attrs.timescale ?? 1))) : 35;
							for (let n = start; n < start + count; n++) segments.push({ url: fill(media, { ...fields, Number: n, Time: n }) });
						}
						if (tpl.attrs.initialization) segments.unshift({ url: fill(join(base, tpl.attrs.initialization), fields), init: true });
					} else if (list) {
						for (const s of kids(list, "SegmentURL")) segments.push({ url: join(base, s.attrs.media) || base, range: s.attrs.mediaRange });
						const init = kid(list, "Initialization");
						if (init) segments.unshift({ url: join(base, init.attrs.sourceURL) || base, range: init.attrs.range, init: true });
					} else if (base) segments.push({ url: base, range: "" });
					const bps = Number(get("bandwidth") ?? 0);
					const track = { id: get("id"), type, segments, bitrate: bitrate(bps), language: get("lang") ?? fallbackLanguage ?? "" };
					if (type === "audio") Object.assign(track, { codec: audioCodec(codecs), channels: Number(kid(rep, "AudioChannelConfiguration")?.attrs.value ?? kid(set, "AudioChannelConfiguration")?.attrs.value) || undefined });
					else if (type === "video") {
						const height = Number(get("height") ?? 0);
						Object.assign(track, { codec: videoCodec(codecs), width: Number(get("width") ?? 0), height, quality: `${height}p`, dynamicRange: "SDR", fps: get("frameRate") });
					} else track.codec = codecs || mime.split("/")[1];
					if (duration && bps) {
						const b = (bps / 8) * duration;
						track.size = { b, kb: b / 1024, mb: b / 1048576, gb: b / 1073741824 };
					}
					all.push(track);
				}
			}
		}
		return JSON.parse(JSON.stringify({ duration, tracks: { all, videos: all.filter((t) => t.type === "video"), audios: all.filter((t) => t.type === "audio"), subtitles: all.filter((t) => t.type === "text") } }));
	};

	const lib = {
		unloads,
		parseDasha,
		libTrace,
		errSignal,
		redux,
		ipcRenderer,
		Album,
		Artist,
		ContentBase,
		ContextMenu,
		MediaItem,
		MediaItems,
		Playlist,
		PlayState,
		Quality,
		StyleTag,
		Tidal,
		TidalApi,
		observe,
		observePromise,
		safeTimeout,
		safeInterval,
		parseDate,
		getCredentials,
		getPlaybackInfo: (id, q) => hostGet("host.playback_info", { id: String(id), quality: q ?? null }),
		// Luna saves a text through the browser's download; here the user
		// picks the file in a save dialog (cancelling saves nothing).
		downloadObject: (content, filename, type) => {
			const name = String(filename ?? "download").replace(/[\/]/g, "_");
			const ext = name.includes(".") ? name.split(".").pop() : "";
			L.fileDialog("save", { title: name, defaultPath: name, filters: ext ? [{ name: `${ext.toUpperCase()} file`, extensions: [ext] }] : [], __content: typeof content === "string" ? content : String(content ?? "") }).catch(L.reportError);
		},
	};
	lib.default = lib;
	core.modules["@luna/lib"] = L.guardModule("@luna/lib", lib);
	L.lib = lib;
	if (L.stat) {
		const host = g.__luna_host;
		g.__luna_host = (kind, json) => {
			const t0 = Date.now();
			try {
				return host(kind, json);
			} finally {
				let what = kind;
				if (kind === "event") {
					try {
						const ev = JSON.parse(json);
						what = `event ${ev.type ?? ev.kind ?? Object.keys(ev)[0]}`;
					} catch {}
				}
				L.stat(`host ${what}`, t0);
			}
		};
	}
})();
