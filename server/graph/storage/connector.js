'use strict';

const _ = require('lodash');

const querystring = require('querystring');

const fetch = require('node-fetch');

const baseLog = require('../../log');

const ROOT_URL = 'https://psychonautwiki.org/w/api.php';

const qsDefaults = {
    action: 'ask',
    format: 'json',
};

/*
    Caching algorithm:

    0. let synchronous be true
    1. check if item is in store
    2. if in store, check ttl
        2.1 let requireRefresh be false
        2.2 let refreshInProgress be false
        2.3 if expired, let requireRefresh be true
        2.4 if refreshInProgress is true
            2.4.1 return existing data; go to 3.
        2.5 let refreshInProgress be true
        2.6 let synchronous be false
        2.7 return existing data
    3. obtain new data, in background if synchronous is false
    3.1 insert new data into cache
    3.2 let requireRefresh be false
    3.3 let refreshInProgress be false
    3.4 return new data if synchronous is true
*/

class BifrostCache {
    constructor({ log }) {
        this._log = log.child({
            type: 'bifrostCache',
        });

        // thirty minutes
        this._CACHE_LIFETIME = 30 * 60 * 1000;

        this._backend = new Map();
        this._processMap = new Map();
    }

    get(key) {
        const cachedItem = this._backend.get(key);

        if (!cachedItem) {
            return null;
        }

        const { ts, val } = cachedItem;

        let requireRefresh = false;

        if ((Date.now() - ts) > this._CACHE_LIFETIME) {
            requireRefresh = true;
        }

        return { val, requireRefresh };
    }

    isBeingRefreshed(key) {
        return this._processMap.get(key) === true;
    }

    markBeingRefreshed(key, isBeingRefreshed) {
        if (!isBeingRefreshed) {
            return this._processMap.delete(key);
        }

        return this._processMap.set(key, true);
    }

    add(key, val) {
        this._log.trace('Adding key: `%s\'', key);

        return this._backend.set(key, {
            ts: Date.now(), val,
        });
    }
}

const sharedBifrostCache = new BifrostCache({
    log: baseLog,
});

const sleepAsync =
    (ms) =>
        new Promise(
            (resolve) =>
                setTimeout(resolve, ms),
        );

class PwConnector {
    constructor({ log }) {
        this._log = log.child({
            type: 'PwConnector',
        });

        this._cache = sharedBifrostCache;
    }

    _fetchUrl(url) {
        this._log.trace(
            'Fetching url: `%s\'',
            url
        );

        let failures = 0;

        return fetch(url)
            .then(res => res.json())
            .catch(err => {
                this._log.error(
                    'Failed to fetch url: `%s\'',
                    url
                );

                failures += 1;

                if (failures > 3) {
                    this._log.error(
                        'Failed 3 times.. \'%s\'',
                        url
                    );

                    throw err;
                }

                this._log.error(
                    'Retrying.. \'%s\'',
                    url
                );

                return sleepAsync(1000 * failures)
                    .then(() => this._fetchUrl(url));
            });
    }

    async _fetchRefreshedCacheItem(url) {
        this._log.trace('Fetching item: `%s`', url);

        try {
            const response = await this._fetchUrl(url);

            this._cache
                .add(
                    url,
                    response,
                );

            return response;
        } catch (err) {
            this._log
                .error(err);

            return null;
        }
    }

    async _getCacheIfNeeded(url) {
        const cacheState = this._cache.get(url);

        /* todo: handle state when key doesnt exist and fetch is in progress */

        if (cacheState === null) {
            return this._fetchRefreshedCacheItem(url);
        }

        const { val, requireRefresh } = cacheState;

        if (requireRefresh && !this._cache.isBeingRefreshed(url)) {
            this._unwindMarkAndRefreshItem(url);

            this._log.trace('Returning expired value of item: `%s`', url);
        }

        return val;
    }

    get(args) {
        const params =
            querystring.encode(
                _.defaults(
                    args,
                    qsDefaults,
                ),
            );

        return this._getCacheIfNeeded(
            `${ROOT_URL}?${params}`,
        );
    }

    _unwindMarkAndRefreshItem(url) {
        this._log.trace('Marking item as being refreshed and unwinding update: `%s`', url);

        /*
         * This method ensures the synchronous marking
         * of an item being refreshed before unwinding
         * the markAndRefreshItem routine from this
         * cycle.
         */

        this._cache.markBeingRefreshed(url, true);

        process.nextTick(() =>
            this._markAndRefreshItemAsync(url),
        );
    }

    async _markAndRefreshItemAsync(url) {
        this._log.trace('[markAndRefreshAsync] Fetching item: `%s`', url);

        await this._fetchRefreshedCacheItem(url);

        this._log.trace('[markAndRefreshAsync] Marking item as not being refreshed: `%s`', url);

        this._cache.markBeingRefreshed(url, false);
    }
}

module.exports = PwConnector;
