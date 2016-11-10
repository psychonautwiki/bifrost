'use strict';

const _ = require('lodash');

class Substances {
    constructor({ connector }) {
        this.connector = connector;
    }

    _mapTextUrl(obj) {
        return _.map(obj, item => {
            const {
                fulltext: name,
                fullurl: url
            } = _.pick(item, ['fulltext', 'fullurl']);

            return {name, url};
        });
    }

    * getSubstances({effect, query, limit, offset}) {
        if (effect && query) {
            throw new Error('Substances: `effect` and `query` are mutually exclusive.');
        }

        /* Delegate the search to a specific substance query */
        if (effect) {
            return yield* this.getEffectSubstances({
                effect, limit, offset
            });
        }

        const articleQuery = query ? `:${query}` : 'Category:Psychoactive substance';

        const res = yield* this.connector.get({
            query: `[[${articleQuery}]]|limit=${limit}|offset=${offset}`
        });

        const results = _.get(res, 'query.results', {});

        return this._mapTextUrl(results);
    }

    * getSubstanceEffects({substance, limit, offset}) {
        const res = yield* this.connector.get({
            query: `[[:${substance}]]|?Effect|limit=${limit}|offset=${offset}`
        });

        const results = _.get(res, `query.results.${substance}.printouts.Effect`, {});

        return this._mapTextUrl(results);
    }

    * getEffects({substance, query, limit, offset}) {
        if (substance && query) {
            throw new Error('Effects: `substance` and `query` are mutually exclusive.');
        }

        /* Delegate the search to a specific substance query */
        if (substance) {
            return yield* this.getSubstanceEffects({
                substance, limit, offset
            });
        }

        const articleQuery = query ? `Effect::${query}` : 'Category:Effect';

        const res = yield* this.connector.get({
            query: `[[${articleQuery}]]|limit=${limit}|offset=${offset}`
        });

        const results = _.get(res, 'query.results', {});

        return this._mapTextUrl(results);
    }

    * getEffectSubstances({effect, limit, offset}) {
        const res = yield* this.connector.get({
            query: `[[Effect::${effect}]]|[[Category:Substance]]|limit=${limit}|offset=${offset}`
        });

        const results = _.get(res, 'query.results', {});

        return this._mapTextUrl(results);
    }
}

module.exports = {
    Substances
};
