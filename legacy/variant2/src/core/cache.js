'use strict';

/**
 * Stale-While-Revalidate In-Memory Cache
 * 
 * Strategy:
 * 1. If key missing: Fetch synchronous (await).
 * 2. If key present & valid: Return value.
 * 3. If key present & expired:
 *    a. Return stale value immediately.
 *    b. Trigger background refresh.
 */
class StaleWhileRevalidateCache {
    constructor({ logger, ttl = 3600000 }) {
        this._logger = logger.child({ component: 'Cache' });
        this._ttl = ttl;
        this._store = new Map();
        this._refreshing = new Map(); // Tracks keys currently being refreshed
    }

    /**
     * Retrieve item from cache or fetch it using the provider.
     * @param {string} key - Cache key
     * @param {Function} fetchProvider - Async function to fetch data if needed
     * @returns {Promise<any>}
     */
    async get(key, fetchProvider) {
        const cached = this._store.get(key);

        // Case 1: Item not in cache
        if (!cached) {
            return this._fetchAndCache(key, fetchProvider);
        }

        const { timestamp, value } = cached;
        const isExpired = (Date.now() - timestamp) > this._ttl;

        // Case 2: Item valid
        if (!isExpired) {
            return value;
        }

        // Case 3: Item expired
        // If already refreshing, just return stale data to avoid thundering herd
        if (this._refreshing.get(key)) {
            this._logger.trace({ key }, 'Returning stale data (refresh already in progress)');
            return value;
        }

        // Trigger background refresh
        this._logger.trace({ key }, 'Returning stale data and triggering background refresh');
        this._refreshing.set(key, true);
        
        // We do not await this. It runs in background.
        this._fetchAndCache(key, fetchProvider)
            .catch(err => {
                this._logger.error({ err, key }, 'Background refresh failed');
            })
            .finally(() => {
                this._refreshing.delete(key);
            });

        return value;
    }

    async _fetchAndCache(key, fetchProvider) {
        this._logger.trace({ key }, 'Fetching fresh data');
        try {
            const value = await fetchProvider();
            this._store.set(key, {
                timestamp: Date.now(),
                value
            });
            return value;
        } catch (err) {
            this._logger.error({ err, key }, 'Fetch failed');
            throw err;
        }
    }
}

module.exports = StaleWhileRevalidateCache;
