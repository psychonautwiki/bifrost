'use strict';

const crypto = require('crypto');

const _ = require('lodash');

const constants = require('../../util/constants');

/*
    ABSTRACT GENERATION
*/

const cheerio = require('cheerio');

class AbstractGenerator {
    static _sanitize(text) {
        return text
            // trim opening paragraph
            .trim()
            // remove reference markers
            .replace(/\[(.*?)]/, '')
            // break by lines
            .split('\n')
            // trim lines -- accounting for \r\n linebreaks
            .map(line => line.trim())
            // take first two paragraphs
            .slice(0, 2)
            // reconcile into single blob
            .join(' ')
            .replace(/\s\s+/);
    }

    static _envelope(extract) {
        const $_base = cheerio.load(`<section>${extract}</section>`);

        return $_base.find('section > p').text();
    }

    static _unwrap(res) {
        const extract = _.get(res, 'parse.text.*', null);

        if (!extract) {
            return null;
        }

        return extract;
    }

    static abstract(res) {
        try {
            return AbstractGenerator._sanitize(AbstractGenerator._envelope(AbstractGenerator._unwrap(res)));
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
    const imageURL = `${cdnURL}w/images/${fileNameHash[0]}/${fileNameHash.slice(0, 2)}/${fileName}`;

    return {
        thumb: imageThumbnail,
        image: imageURL,
    };
};

class Substances {
    constructor({ connector, pwPropParser, log }) {
        this._connector = connector;
        this._pwPropParser = pwPropParser;

        // special controllers
        this._abstractGenerator = new AbstractGenerator();

        this._log = log.child({
            type: 'Substances',
        });
    }

    _mapTextUrl(obj) {
        return _.map(obj, item => {
            const {
                fulltext: name,
                fullurl: url,
            } = _.pick(item, ['fulltext', 'fullurl']);

            return { name, url };
        });
    }

    static _renderPagination({ limit, offset }) {
        return `${limit ? `|limit=${limit}` : ''}${offset ? `|offset=${offset}` : ''}`;
    }

    async getSemanticSubstanceProps(substance) {
        this._log.trace('[getSemanticSubstanceProps] substance: %s', substance);

        const res = await this._connector.get({
            action: 'browsebysubject',
            subject: substance,
        });

        return this._pwPropParser
            .parseFromSMW(res);
    }

    async getSubstances({ chemicalClass, psychoactiveClass, effect, query, limit, offset }) {
        if ([effect, query, chemicalClass, psychoactiveClass].filter(a => a).length >= 2) {
            throw new Error('Substances: `chemicalClass`, `psychoactiveClass`, `effect` and `query` are mutually exclusive.');
        }

        this._log.trace('[getSubstances] effect: %s query: %s chemicalClass: %s psychoactiveClass: %s', effect, query, psychoactiveClass);

        /* delegate to chemicalClass search */
        if (chemicalClass) {
            return this.getChemicalClassSubstances({
                chemicalClass, limit, offset,
            });
        }

        /* delegate to psychoactiveClass search */
        if (psychoactiveClass) {
            return this.getPsychoactiveClassSubstances({
                psychoactiveClass, limit, offset,
            });
        }

        /* Delegate the search to a specific substance query */
        if (effect) {
            return this.getEffectSubstances({
                effect, limit, offset,
            });
        }

        const articleQuery = query ? `:${query}` : 'Category:Psychoactive substance';

        const res = await this._connector.get({
            query: `[[${articleQuery}]]${Substances._renderPagination({ limit, offset })}`,
        });

        const results = _.get(res, 'query.results', {});

        const self = this;

        return Promise.all(
            this._mapTextUrl(results)
                .map(async item => {
                    const semanticData =
                        await this.getSemanticSubstanceProps(
                            item.name,
                        );

                    process.env.DUMP_SEMANTICS && this._log.trace('Processed semantic data', semanticData);

                    return _.merge(item, semanticData);
                }),
        );
    }

    async getSubstanceEffects({ substance, limit, offset }) {
        this._log.trace('[getSubstanceEffects] substance: %s', substance);

        const res = await this._connector.get({
            query: `[[:${substance}]]|?Effect${Substances._renderPagination({ limit, offset })}`,
        });

        const results = _.get(res, `query.results.${substance}.printouts.Effect`, {});

        return this._mapTextUrl(results);
    }

    async getSubstanceAbstract({ substance }) {
        this._log.trace('[getSubstanceAbstract] substance: %s', substance);

        const abstractPayload = await this._connector.get({
            action: 'parse',
            page: substance,
            prop: 'text',
            section: 0,
        });

        const targetSummary = AbstractGenerator.abstract(abstractPayload);

        this._log.trace('[getSubstanceAbstract:result] %s', targetSummary);

        return targetSummary;
    }

    async getSubstanceImages({ substance }) {
        this._log.trace('[getSubstanceImages] substance: %s', substance);

        const imagePayload = await this._connector.get({
            action: 'parse',
            page: substance,
            prop: 'images',
        });

        const images = _.get(imagePayload, 'parse.images', null);

        this._log.trace('[getSubstanceImages:result] %s', images);

        if (!_.isArray(images)) {
            return null;
        }

        return images.map(buildImage);
    }

    async getEffects({ substance, query, limit, offset }) {
        if (substance && query) {
            throw new Error('Effects: `substance` and `query` are mutually exclusive.');
        }

        this._log.trace('[getEffects] substance: %s query: %s', substance, query);

        /* Delegate the search to a specific substance query */
        if (substance) {
            return this.getSubstanceEffects({
                substance, limit, offset,
            });
        }

        const articleQuery = query ? `Effect::${query}` : 'Category:Effect';

        const res = await this._connector.get({
            query: `[[${articleQuery}]]${Substances._renderPagination({ limit, offset })}`,
        });

        const results = _.get(res, 'query.results', {});

        return this._mapTextUrl(results);
    }

    async getEffectSubstances({ effect, limit, offset }) {
        this._log.trace('[getEffectSubstances] effect: %s', effect);

        const serializedEffectQuery = effect.map(effectName => `[[Effect::${effectName}]]`).join('|');

        const res = await this._connector.get({
            query: `${serializedEffectQuery}|[[Category:Psychoactive substance]]${Substances._renderPagination({
                limit,
                offset,
            })}`,
        });

        const results = _.get(res, 'query.results', {});

        return this._mapTextUrl(results);
    }

    async getChemicalClassSubstances({ chemicalClass, limit, offset }) {
        this._log.trace('[getChemicalClassSubstances] effect: %s', chemicalClass);

        const res = await this._connector.get({
            query: `[[Chemical class::${chemicalClass}]]|[[Category:Psychoactive substance]]${Substances._renderPagination({
                limit,
                offset,
            })}`,
        });

        const results = _.get(res, 'query.results', {});

        return this._mapTextUrl(results);
    }

    async getPsychoactiveClassSubstances({ psychoactiveClass, limit, offset }) {
        this._log.trace('[getPsychoactiveClassSubstances] effect: %s', psychoactiveClass);

        const res = await this._connector.get({
            query: `[[Psychoactive class::${psychoactiveClass}]]|[[Category:Psychoactive substance]]${Substances._renderPagination({
                limit,
                offset,
            })}`,
        });

        const results = _.get(res, 'query.results', {});

        return this._mapTextUrl(results);
    }
}

module.exports = {
    Substances,
};
