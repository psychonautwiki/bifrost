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

class BifrostCache {
    constructor({log}) {
        this._log = log.child({
            type: 'bifrostCache'
        });

        this.backend = new LruMap({}, 1024);
    }

    getExpireIfNeeded(key, ttl) {
        this._log.trace('Looking up key: `%s`', key);

        const item = this.backend.get(key);

        if (!item) {
            return null;
        }

        if ((Date.now() - item.ts) > ttl) {
            this._log.trace('Key invalidated, removing: `%s` (ttl: %s)', key, Date.now() - item.ts);

            this.backend.delete(key);

            return null;
        }

        return item.val;
    }

    add(key, val) {
        this._log.trace('Adding key: `%s`', key);

        return this.backend.set(key, {
            ts: Date.now(), val
        });
    }
}

const sharedBifrostCache = new BifrostCache({
    log: baseLog
});

class PwConnector {
    constructor({log}) {
        // two minutes
        this.LRU_LIFETIME = 2 * 60 * 1000;

        this._rp = rp;
        this._log = log.child({
            type: 'PwConnector'
        });

        this._loader = new DataLoader(this.fetch.bind(this), {
            batch: true
        });

        this._cache = sharedBifrostCache;
    }

    fetch(urls) {
        const options = {
            json: true,
            resolveWithFullResponse: true,
            headers: {
                'user-agent': 'Bifrost'
            }
        };

        const ctx = this;

        this.ts = Date.now();
        return Promise.all(
            urls.map(
                Promise.coroutine(function* (url) {
                    try {
                        ctx._log.debug('Trying to load url: `%s`', url);

//                        const cacheItem = ctx._cache.getExpireIfNeeded(url, ctx.LRU_LIFETIME);

//                        if (cacheItem) {
//                            return cacheItem;
//                        }

                        const response = yield ctx._rp(
                            _.assign({
                                uri: url
                            }, options)
                        );

//                        ctx._cache.add(url, response.body);

                        return response.body;
                    } catch (err) {
                        ctx._log.debug('Failed to load url: `%s`', err.message);

                        throw err;
                    }
                })
            )
        );
    }

    *get(args) {
        const params = querystring.encode(_.defaults(args, qsDefaults));


        const val = yield this._loader.load(`${ROOT_URL}?${params}`);

        return val;
    }
}

module.exports = PwConnector;
