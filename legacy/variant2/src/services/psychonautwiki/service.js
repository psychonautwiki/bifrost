'use strict';

const _ = require('lodash');
const cheerio = require('cheerio');
const crypto = require('crypto');

class PsychonautWikiService {
    constructor({ connector, parser, config, logger }) {
        this._connector = connector;
        this._parser = parser;
        this._config = config;
        this._logger = logger.child({ component: 'PWService' });
    }

    async getSubstances({ chemicalClass, psychoactiveClass, effect, query, limit, offset }) {
        // Mutually exclusive check
        if ([effect, query, chemicalClass, psychoactiveClass].filter(Boolean).length >= 2) {
            throw new Error('Substances: `chemicalClass`, `psychoactiveClass`, `effect` and `query` are mutually exclusive.');
        }

        if (chemicalClass) return this._getByClass('Chemical class', chemicalClass, limit, offset);
        if (psychoactiveClass) return this._getByClass('Psychoactive class', psychoactiveClass, limit, offset);
        if (effect) return this.getEffectSubstances({ effect: [effect], limit, offset });

        // Default query search
        const articleQuery = query ? `:${query}` : 'Category:Psychoactive substance';
        
        // Try direct lookup
        let results = await this._lookupWithPagination(`[[${articleQuery}]]`, limit, offset);

        // Try common name
        if (_.isEmpty(results)) {
            results = await this._lookupWithPagination(`[[common_name::${query}]]|[[Category:psychoactive_substance]]`, limit, offset);
        }

        // Try systematic name
        if (_.isEmpty(results)) {
            results = await this._lookupWithPagination(`[[systematic_name::${query}]]|[[Category:psychoactive_substance]]`, limit, offset);
        }

        return this._enrichResults(results);
    }

    async getSubstanceEffects({ substance, limit, offset }) {
        const res = await this._connector.get({
            query: `[[:${substance}]]|?Effect${this._renderPagination(limit, offset)}`
        });
        const results = _.get(res, `query.results.${substance}.printouts.Effect`, {});
        return this._mapTextUrl(results);
    }

    async getEffectSubstances({ effect, limit, offset }) {
        const effects = Array.isArray(effect) ? effect : [effect];
        const query = effects.map(e => `[[Effect::${e}]]`).join('|');
        
        const res = await this._connector.get({
            query: `${query}|[[Category:Psychoactive substance]]${this._renderPagination(limit, offset)}`
        });
        
        return this._mapTextUrl(_.get(res, 'query.results', {}));
    }

    async getSubstanceAbstract({ substance }) {
        const res = await this._connector.get({
            action: 'parse',
            page: substance,
            prop: 'text',
            section: 0
        });

        try {
            const html = _.get(res, 'parse.text.*');
            if (!html) return null;

            const $ = cheerio.load(`<section>${html}</section>`);
            const text = $('section > p').text();
            
            return text
                .trim()
                .replace(/\[(.*?)]/, '') // remove refs
                .split('\n')
                .map(l => l.trim())
                .slice(0, 2)
                .join(' ')
                .replace(/\s\s+/, ' ');
        } catch (err) {
            this._logger.error({ err }, 'Failed to parse abstract');
            return null;
        }
    }

    async getSubstanceImages({ substance }) {
        const res = await this._connector.get({
            action: 'parse',
            page: substance,
            prop: 'images'
        });

        const images = _.get(res, 'parse.images');
        if (!Array.isArray(images)) return null;

        const { cdnUrl, thumbSize } = this._config.psychonautWiki;

        return images.map(fileName => {
            const hash = crypto.createHash('md5').update(fileName).digest('hex');
            return {
                thumb: `${cdnUrl}w/thumb.php?f=${fileName}&width=${thumbSize}`,
                image: `${cdnUrl}w/images/${hash[0]}/${hash.slice(0, 2)}/${fileName}`
            };
        });
    }

    // Helpers

    async _getByClass(classType, className, limit, offset) {
        const res = await this._connector.get({
            query: `[[${classType}::${className}]]|[[Category:Psychoactive substance]]${this._renderPagination(limit, offset)}`
        });
        return this._mapTextUrl(_.get(res, 'query.results', {}));
    }

    async _lookupWithPagination(query, limit, offset) {
        const res = await this._connector.get({
            query: `${query}${this._renderPagination(limit, offset)}`
        });
        return _.get(res, 'query.results', {});
    }

    async _enrichResults(results) {
        const items = this._mapTextUrl(results);
        
        return Promise.all(items.map(async item => {
            const semanticData = await this._getSemanticProps(item.name);
            return _.merge(item, semanticData);
        }));
    }

    async _getSemanticProps(substance) {
        const res = await this._connector.get({
            action: 'browsebysubject',
            subject: substance
        });
        return this._parser.parseFromSMW(res);
    }

    _renderPagination(limit, offset) {
        return `${limit ? `|limit=${limit}` : ''}${offset ? `|offset=${offset}` : ''}`;
    }

    _mapTextUrl(obj) {
        return _.map(obj, item => ({
            name: item.fulltext,
            url: item.fullurl
        }));
    }
}

module.exports = PsychonautWikiService;
