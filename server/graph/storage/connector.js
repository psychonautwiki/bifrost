'use strict';

const _ = require('lodash');
const Promise = require('bluebird');

const querystring = require('querystring');

const rp = require('request-promise');
const DataLoader = require('dataloader');

const baseLog = require('../../log');

const LruMap = require('collections/lru-map');

const ROOT_URL = 'https://psychonautwiki.org/w/api.php';

const qsDefaults = {
    action: 'ask',
    format: 'json'
};

/*
    Caching algorithm:

    1. check if item is in store
    2. if in store, check ttl
        2.1 let requireRefresh be false
        2.2 let refreshInProgress be false
        2.3 if expired, let requireRefresh be true
        2.4 if refreshInProgress is true
            2.4.1 return existing data; bail
        2.5 return existing data
        2.6 let refreshInProgress be true
    3. obtain new data in background
    3.1 let requireRefresh be false
    3.2 let refreshInProgress be false
*/

class BifrostCache {
    constructor({log}) {
        this._log = log.child({
            type: 'bifrostCache'
        });

        // thirty minutes
        this._CACHE_LIFETIME = 30 * 60 * 1000;

        this._backend = new Map();
        this._processMap = new Map();
    }

    /* this._log.trace('Key invalidated, removing: `%s` (ttl: %s)', key, Date.now() - item.ts); */

    get(key) {
        const cachedItem = this._backend.get(key);

        if (!cachedItem) {
            return null;
        }

        const {ts, val} = cachedItem;

        let requireRefresh = false;

        if ((Date.now() - ts) > this._CACHE_LIFETIME) {
            requireRefresh = true;
        }

        return {val, requireRefresh};
    }

    isBeingRefreshed(key) {
        return this._processMap.get(key) !== undefined;
    }

    markBeingRefreshed(key, isBeingRefreshed) {
        if (!isBeingRefreshed) {
            return this._processMap.delete(key);
        }

        return this._processMap.set(key, true);
    }

    add(key, val) {
        this._log.trace('Adding key: `%s`', key);

        return this._backend.set(key, {
            ts: Date.now(), val
        });
    }
}

const sharedBifrostCache = new BifrostCache({
    log: baseLog
});

class PwConnector {
    constructor({log}) {
        this._rp = rp;
        this._log = log.child({
            type: 'PwConnector'
        });

        this._loader = new DataLoader(this.fetch.bind(this), {
            batch: true
        });

        this._cache = sharedBifrostCache;

        this._fetchOptions = {
            json: true,
            resolveWithFullResponse: true,
            headers: {
                'user-agent': 'Bifrost'
            }
        };
    }

    _buildFetchOptions(url) {
        return _.assign({
            uri: url
        }, this._fetchOptions);
    }

    _fetchUrl(url) {
        return this._rp(this._buildFetchOptions(url));
    }

    * _fetchRefreshedCacheItem(url) {
        const response = yield this._fetchUrl(url);

        this._cache.add(url, response);

        return response;
    }

    * _getCacheIfNeeded(url) {
        const cacheState = this._cache.get(url);

        /* todo: handle state when key doesnt exist and fetch is in progress */

        if (cacheState === null) {
            const res = yield* this._fetchRefreshedCacheItem(url);

            return res.body;
        }

        const {val, requireRefresh} = cacheState;

        if (requireRefresh && !this._cache.isBeingRefreshed(url)) {
            this._cache.markBeingRefreshed(url, true);

            const newVal = yield* this._fetchRefreshedCacheItem(url);

            this._cache.markBeingRefreshed(url, false);

            return newVal.body;
        }

        return val.body;
    }

    fetch(urls) {
        return Promise.all(urls.map(url =>
            this._loadUrl(url)
        ));
    }

    * get(args) {
        const params = querystring.encode(_.defaults(args, qsDefaults));

        return yield this._loader.load(`${ROOT_URL}?${params}`);
    }
}

PwConnector.prototype._loadUrl = Promise.coroutine(function* (url) {
    try {
        this._log.debug('Trying to load url: `%s`', url);

        return yield* this._getCacheIfNeeded(url);
    } catch (err) {
        this._log.debug('Failed to load url: `%s`', err.message);

        throw err;
    }
});

module.exports = PwConnector;
