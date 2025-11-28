'use strict';

const _ = require('lodash');

const REGEX = {
    /* durations */
    range_dur: /(.*?)_(.*?)_(.*?)_time$/i,

    /* doses */
    range_dose: /(.*?)_(.*?)_(.*?)_dose$/i,
    def_dose: /(.*?)_(.*?)_dose$/i,

    /* bioavailability */
    def_bioavailability: /(.*?)_(.*?)_bioavailability$/i,

    /* units */
    dose_units: /(.*?)_dose_units$/i,
    roa_time_units: /(.*?)_(.*?)_time_units$/i,

    /* meta: tolerance */
    meta_tolerance_time: /Time_to_(.*?)_tolerance$/i,

    /* misc */
    wt_prop_glob: /\[\[(.*?)]]/g,
    wt_prop: /\[\[(.*?)]]/,

    /* misc sanitization */
    wt_link: /(\[\[.*?]])/ig,
    wt_named_link: /(\[\[.*?\|.*?]])/ig,
    wt_sub_sup: /<su[bp]>(.*?)<\/su[bp]>/ig,
};

class WikitextParser {
    constructor({ smwTransformer }) {
        this._smwTransformer = smwTransformer;
        
        this._flatMetaProps = new Map([
            ['addiction_potential', 'addictionPotential'],
            ['uncertaininteraction', 'uncertainInteractions'],
            ['unsafeinteraction', 'unsafeInteractions'],
            ['dangerousinteraction', 'dangerousInteractions'],
            ['effect', 'effects'],
            ['common_name', 'commonNames'],
            ['systematic_name', 'systematicName'],
        ]);

        this._mappedMetaProps = new Map([
            ['cross-tolerance', this._mapCrossTolerance.bind(this)],
            ['featured', prop => ['featured', prop === 't']],
            ['toxicity', prop => (['toxicity', [].concat(prop)])],
            ['psychoactive_class', prop => this._mapClass('class.psychoactive', prop)],
            ['chemical_class', prop => this._mapClass('class.chemical', prop)],
            ['common_name', prop => this._mapClass('commonNames', prop)],
        ]);

        const forceArray = val => Array.isArray(val) ? val : [val];

        this._sanitizers = new Map([
            ['addictionPotential', val => this._sanitizeText(val)],
            ['toxicity', val => val.map(item => this._sanitizeText(item))],
            ['uncertainInteractions', forceArray],
            ['unsafeInteractions', forceArray],
            ['dangerousInteractions', forceArray],
            ['effects', forceArray],
            ['commonNames', forceArray],
            ['systematicName', val => this._sanitizeText(val)],
        ]);
    }

    parseFromSMW(obj) {
        const parsedSMW = this._smwTransformer.parse(obj);
        return this._processProperties(parsedSMW);
    }

    _processProperties(propSet) {
        const procPropMap = {};

        // propSet is [subject, [[propName, {type, prop}], ...]]
        propSet[1].forEach(([_propName, { prop }]) => {
            const propName = _propName.toLowerCase();
            this._dispatchProperty(procPropMap, propName, prop);
        });

        // Post-process ROAs
        const rawROAMap = _.get(procPropMap, 'roa', {});
        const mappedROAs = _.chain(rawROAMap)
            .keys()
            .map(key => _.merge(_.get(rawROAMap, key), { name: key }))
            .value();

        _.assign(procPropMap, { roas: mappedROAs });

        return procPropMap;
    }

    _dispatchProperty(map, propName, propValue) {
        let rx;

        // Durations
        if ((rx = REGEX.range_dur.exec(propName))) {
            _.set(map, `roa.${rx[1]}.duration.${rx[3]}.${rx[2]}`, propValue);
            return;
        }

        // Doses
        if ((rx = REGEX.range_dose.exec(propName))) {
            _.set(map, `roa.${rx[1]}.dose.${rx[3]}.${rx[2]}`, propValue);
            return;
        }
        if ((rx = REGEX.def_dose.exec(propName))) {
            _.set(map, `roa.${rx[1]}.dose.${rx[2]}`, propValue);
            return;
        }

        // Bioavailability
        if ((rx = REGEX.def_bioavailability.exec(propName))) {
            _.set(map, `roa.${rx[1]}.bioavailability.${rx[2]}`, propValue);
            return;
        }

        // Units
        if ((rx = REGEX.dose_units.exec(propName))) {
            _.set(map, `roa.${rx[1]}.dose.units`, propValue);
            return;
        }
        if ((rx = REGEX.roa_time_units.exec(propName))) {
            _.set(map, `roa.${rx[1]}.duration.${rx[2]}.units`, propValue);
            return;
        }

        // Meta: Tolerance
        if ((rx = REGEX.meta_tolerance_time.exec(propName))) {
            _.set(map, `tolerance.${rx[1]}`, propValue);
            return;
        }

        // Flat Props
        if (this._flatMetaProps.has(propName)) {
            const target = this._flatMetaProps.get(propName);
            _.set(map, target, this._sanitizeIfNeeded(target, propValue));
        }

        // Mapped Props
        if (this._mappedMetaProps.has(propName)) {
            try {
                const [target, val] = this._mappedMetaProps.get(propName)(propValue);
                _.set(map, target, this._sanitizeIfNeeded(target, val));
            } catch (e) {
                // Fallback
                _.set(map, propName, this._sanitizeIfNeeded(propName, propValue));
            }
        }
    }

    _mapCrossTolerance(prop) {
        const target = 'crossTolerances';
        if (!REGEX.wt_prop_glob.test(prop)) return [target, []];
        
        const matches = prop.match(REGEX.wt_prop_glob) || [];
        const mapped = matches.map(item => REGEX.wt_prop.exec(item)[1]);
        return [target, mapped];
    }

    _mapClass(target, prop) {
        return [
            target,
            [].concat(prop).map(p => p.replace(/#$/, '').replace(/_/g, ' '))
        ];
    }

    _sanitizeIfNeeded(propName, propValue) {
        if (!propValue) return propValue;
        if (this._sanitizers.has(propName)) {
            return this._sanitizers.get(propName)(propValue);
        }
        return propValue;
    }

    _sanitizeText(text) {
        if (!text || typeof text !== 'string') return text;
        
        let out = text;

        // Handle [[link|name]]
        const namedLinks = out.match(REGEX.wt_named_link) || [];
        for (const match of namedLinks) {
            const content = match.slice(2, -2).split('|').pop();
            out = out.replace(match, content);
        }

        // Handle [[link]]
        const links = out.match(REGEX.wt_link) || [];
        for (const match of links) {
            const content = match.slice(2, -2);
            out = out.replace(match, content);
        }

        // Handle sub/sup
        out = out.replace(REGEX.wt_sub_sup, '$1');

        return out;
    }
}

module.exports = WikitextParser;
