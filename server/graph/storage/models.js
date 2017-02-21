'use strict';

const Promise = require('bluebird');
const _ = require('lodash');

class Substances {
    constructor({connector, pwPropParser, log}) {
        this._connector = connector;
        this._pwPropParser = pwPropParser;

        this._log = log.child({
            type: 'Substances'
        });
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

    _renderPagination({limit, offset}) {
        return `${limit ? `|limit=${limit}` : ''}${offset ? `|offset=${offset}` : ''}`;
    }

    * getSemanticSubstanceProps(substance) {
        this._log.trace('[getSemanticSubstanceProps] substance: %s', substance);

        const res = yield* this._connector.get({
            action: 'browsebysubject',
            subject: substance
        });

        return this._pwPropParser.parseFromSMW(res);
    }

    * getSubstances({effect, query, limit, offset}) {
        if (effect && query) {
            throw new Error('Substances: `effect` and `query` are mutually exclusive.');
        }

        this._log.trace('[getSubstances] effect: %s query: %s', effect, query);

        /* Delegate the search to a specific substance query */
        if (effect) {
            return yield* this.getEffectSubstances({
                effect, limit, offset
            });
        }

        const articleQuery = query ? `:${query}` : 'Category:Psychoactive substance';

        const res = yield* this._connector.get({
            query: `[[${articleQuery}]]${this._renderPagination({limit, offset})}`
        });

        const results = _.get(res, 'query.results', {});

        const mappedResults = yield Promise.all(
            this._mapTextUrl(results).map(item =>
                Promise.coroutine(function* (_item) {
                    const semanticData = yield* this.getSemanticSubstanceProps(_item.name);

                    process.env.DUMP_SEMANTICS && this._log.trace('Processed semantic data', semanticData);

                    return _.merge(item, semanticData);
                }).call(this, item)
            )
        );

        console.log(require('util').inspect(mappedResults, {depth:null}));

        return mappedResults;
    }

    * getSubstanceEffects({substance, limit, offset}) {
        this._log.trace('[getSubstanceEffects] substance: %s', substance);

        const res = yield* this._connector.get({
            query: `[[:${substance}]]|?Effect${this._renderPagination({limit, offset})}`
        });

        const results = _.get(res, `query.results.${substance}.printouts.Effect`, {});

        return this._mapTextUrl(results);
    }

    * getEffects({substance, query, limit, offset}) {
        if (substance && query) {
            throw new Error('Effects: `substance` and `query` are mutually exclusive.');
        }

        this._log.trace('[getEffects] substance: %s query: %s', substance, query);

        /* Delegate the search to a specific substance query */
        if (substance) {
            return yield* this.getSubstanceEffects({
                substance, limit, offset
            });
        }

        const articleQuery = query ? `Effect::${query}` : 'Category:Effect';

        const res = yield* this._connector.get({
            query: `[[${articleQuery}]]${this._renderPagination({limit, offset})}`
        });

        const results = _.get(res, 'query.results', {});

        return this._mapTextUrl(results);
    }

    * getEffectSubstances({effect, limit, offset}) {
        this._log.trace('[getEffectSubstances] effect: %s', effect);

        const res = yield* this._connector.get({
            query: `[[Effect::${effect}]]|[[Category:Substance]]${this._renderPagination({limit, offset})}`
        });

        const results = _.get(res, 'query.results', {});

        return this._mapTextUrl(results);
    }
}

module.exports = {
    Substances
};
