'use strict';

const _ = require('lodash');
const Promise = require('bluebird');

const querystring = require('querystring');

const rp = require('request-promise');
const DataLoader = require('dataloader');

const ROOT_URL = 'https://psychonautwiki.org/w/api.php';

const qsDefaults = {
    action: 'ask',
    format: 'json'
};

const eTagCache = {};

class PwConnector {
    constructor() {
        this.rp = rp;

        this.loader = new DataLoader(this.fetch.bind(this), {
            batch: false
        });
    }

    fetch(urls) {
        const options = {
            json: true,
            resolveWithFullResponse: true,
            headers: {
                'user-agent': 'GitHunt'
            }
        };

        const rp = this.rp;

        return Promise.all(urls.map(url => {
            const cachedRes = eTagCache[url];

            if (cachedRes && cachedRes.eTag) {
                options.headers['If-None-Match'] = cachedRes.eTag;
            }

            return Promise.coroutine(function* () {
                try {
                    const response = yield rp(
                        _.assign({
                            uri: url
                        }, options)
                    );

                    const body = response.body;

                    eTagCache[url] = {
                        result: body,
                        eTag: response.headers.etag
                    };

                    return body;
                } catch (err) {
                    if (err.statusCode !== 304) {
                        throw err;
                    }

                    return cachedRes.result;
                }
            })();
        }));
    }

    *get(args) {
        const params = querystring.encode(_.defaults(args, qsDefaults));

        return yield this.loader.load(`${ROOT_URL}?${params}`);
    }
}

module.exports = PwConnector;
