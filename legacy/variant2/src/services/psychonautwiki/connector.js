'use strict';

const fetch = require('node-fetch');
const querystring = require('querystring');
const _ = require('lodash');

class PsychonautWikiConnector {
    constructor({ config, logger, cache }) {
        this._config = config;
        this._logger = logger.child({ component: 'PWConnector' });
        this._cache = cache;
        this._baseUrl = config.psychonautWiki.apiUrl;
    }

    /**
     * Fetch data from PW API with caching and retries
     * @param {Object} params - Query parameters
     * @returns {Promise<Object>}
     */
    async get(params) {
        const qs = querystring.encode(_.defaults(params, {
            action: 'ask',
            format: 'json'
        }));

        const url = `${this._baseUrl}?${qs}`;

        return this._cache.get(url, () => this._fetchWithRetry(url));
    }

    async _fetchWithRetry(url, retries = 3) {
        this._logger.trace({ url }, 'Fetching URL');

        console.log(url);

        for (let i = 0; i <= retries; i++) {
            try {
                const res = await fetch(url);
                if (!res.ok) throw new Error(`HTTP ${res.status}`);
                return await res.json();
            } catch (err) {
                if (i === retries) {
                    this._logger.error({ err, url }, 'Fetch failed permanently');
                    throw err;
                }

                const delay = 1000 * (i + 1);
                this._logger.warn({ err, url, attempt: i + 1 }, `Fetch failed, retrying in ${delay}ms`);
                await new Promise(resolve => setTimeout(resolve, delay));
            }
        }
    }
}

module.exports = PsychonautWikiConnector;
