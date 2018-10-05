'use strict';

const crypto = require('crypto');

const Promise = require('bluebird');
const _ = require('lodash');

const constants = require('../../util/constants');

/*
    ABSTRACT GENERATION
*/

const cheerio = require('cheerio');

class AbstractGenerator {
    _sanitize(text) {
        return text
            // trim opening paragraph
            .trim()
            // remove reference markers
            .replace(/\[(.*?)\]/, '')
            // break by lines
            .split('\n')
            // trim lines -- accounting for \r\n linebreaks
            .map(line => line.trim())
            // take first two paragraphs
            .slice(0,2)
            // reconcile into single blob
            .join(' ')
            .replace(/\s\s+/);
    }

    _envelope(extract) {
        const $_base = cheerio(`<section>${extract}</section>`);

        return $_base.find('section > p').text();
    }

    _unwrap(res) {
        const extract = _.get(res, 'parse.text.*', null);

        if (!extract) {
            return null;
        }

        return extract;
    }

    abstract(res) {
        try {
            return this._sanitize(this._envelope(this._unwrap(res)));
        } catch (err) {
            return err;
        }
    }
}

/*
    IMAGE URL COMPUTATION
*/

const cdnURL = constants.get('cdn');
const thumbSize = constants.get('thumbSize');

const buildImage = fileName => {
    const fileNameHash = crypto.createHash('md5')
                               .update(fileName)
                               .digest()
                               .toString('hex');

    const imageThumbnail = `${cdnURL}w/thumb.php?f=${fileName}&width=${thumbSize}`;
    const imageURL = `${cdnURL}w/images/${fileNameHash[0]}/${fileNameHash.slice(0,2)}/${fileName}`;

    return {
        thumb: imageThumbnail,
        image: imageURL
    };
};

class Substances {
    constructor({connector, pwPropParser, log}) {
        this._connector = connector;
        this._pwPropParser = pwPropParser;

        // special controllers
        this._abstractGenerator = new AbstractGenerator();

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

    * getSubstances({chemicalClass, psychoactiveClass, effect, query, limit, offset}) {
        if ([effect, query, chemicalClass, psychoactiveClass].filter(a => a).length >= 2) {
            throw new Error('Substances: `chemicalClass`, `psychoactiveClass`, `effect` and `query` are mutually exclusive.');
        }

        this._log.trace('[getSubstances] effect: %s query: %s chemicalClass: %s psychoactiveClass: %s', effect, query, psychoactiveClass);

        /* delegate to chemicalClass search */
        if (chemicalClass) {
            return yield* this.getChemicalClassSubstances({
                chemicalClass, limit, offset
            });
        }

        /* delegate to psychoactiveClass search */
        if (psychoactiveClass) {
            return yield* this.getPsychoactiveClassSubstances({
                psychoactiveClass, limit, offset
            });
        }

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

    * getSubstanceAbstract({substance}) {
        this._log.trace('[getSubstanceAbstract] substance: %s', substance);

        const abstractPayload = yield* this._connector.get({
            action: 'parse',
            page: substance,
            prop: 'text',
            section: 0
        });

        const targetSummary = this._abstractGenerator.abstract(abstractPayload);

        this._log.trace('[getSubstanceAbstract:result] %s', targetSummary);

        return targetSummary;
    }

    * getSubstanceImages({substance}) {
        this._log.trace('[getSubstanceImages] substance: %s', substance);

        const imagePayload = yield* this._connector.get({
            action: 'parse',
            page: substance,
            prop: 'images'
        });

        const images = _.get(imagePayload, 'parse.images', null);

        this._log.trace('[getSubstanceImages:result] %s', images);

        if (!_.isArray(images)) {
            return null;
        }

        return images.map(buildImage);
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

        const serializedEffectQuery = effect.map(effectName => `[[Effect::${effectName}]]`).join('|');

        const res = yield* this._connector.get({
            query: `${serializedEffectQuery}|[[Category:Psychoactive substance]]${this._renderPagination({limit, offset})}`
        });

        const results = _.get(res, 'query.results', {});

        return this._mapTextUrl(results);
    }

    * getChemicalClassSubstances({chemicalClass, limit, offset}) {
        this._log.trace('[getChemicalClassSubstances] effect: %s', chemicalClass);

        const res = yield* this._connector.get({
            query: `[[Chemical class::${chemicalClass}]]|[[Category:Psychoactive substance]]${this._renderPagination({limit, offset})}`
        });

        const results = _.get(res, 'query.results', {});

        return this._mapTextUrl(results);
    }

    * getPsychoactiveClassSubstances({psychoactiveClass, limit, offset}) {
        this._log.trace('[getPsychoactiveClassSubstances] effect: %s', psychoactiveClass);

        const res = yield* this._connector.get({
            query: `[[Psychoactive class::${psychoactiveClass}]]|[[Category:Psychoactive substance]]${this._renderPagination({limit, offset})}`
        });

        const results = _.get(res, 'query.results', {});

        return this._mapTextUrl(results);
    }
}

module.exports = {
    Substances
};
