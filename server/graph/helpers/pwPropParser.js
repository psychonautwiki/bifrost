'use strict';

const _ = require('lodash');

class PWPropParser {
    constructor({ smwDataArbitrator }) {
        this._smwDataArbitrator = smwDataArbitrator;

        this._rgx = {
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
            ['cross-tolerance', prop => {
                const propTarget = 'crossTolerances';

                if (!this._rgx.wt_prop_glob.test(prop)) {
                    return [propTarget, []];
                }

                const mappedCrossTolerance = Array.prototype.slice
                    .call(prop.match(this._rgx.wt_prop_glob))
                    .map(item =>
                        this._rgx.wt_prop.exec(item)[1],
                    );

                return [propTarget, mappedCrossTolerance];
            }],
            ['featured', prop => ['featured', prop === 't']],
            ['toxicity', prop => (['toxicity', [].concat(prop)])],
            [
                'psychoactive_class',
                prop => ([
                    'class.psychoactive',
                    []
                        .concat(prop)
                        .map(prop =>
                            prop.replace(/#$/, '')
                                .replace(/_/g, ' '),
                        ),
                ]),
            ],
            [
                'chemical_class',
                prop => ([
                    'class.chemical',
                    []
                        .concat(prop)
                        .map(prop =>
                            prop.replace(/#$/, '')
                                .replace(/_/g, ' '),
                        ),
                ]),
            ],
            [
                'common_name',
                prop => ([
                    'commonNames',
                    []
                        .concat(prop)
                        .map(prop =>
                            prop.replace(/#$/, '')
                                .replace(/_/g, ' '),
                        ),
                ]),
            ],
        ]);

        const forceArray = val => Array.isArray(val) ? val : [val];

        this._sanitizers = new Map([
            [
                'addictionPotential',
                val => this._sanitizeText(val),
            ],
            [
                'toxicity',
                val => val.map(item => this._sanitizeText(item)),
            ],
            [
                'uncertainInteractions',
                forceArray,
            ],
            [
                'unsafeInteractions',
                forceArray,
            ],
            [
                'dangerousInteractions',
                forceArray,
            ],
            [
                'effects',
                forceArray,
            ],
            [
                'commonNames',
                forceArray,
            ],
            [
                'systematicName',
                val => this._sanitizeText(val),
            ],
        ]);
    }

    _sanitizeText(propValue) {
        let tmpVal = propValue;

        if (!tmpVal) {
            return tmpVal;
        }

        if (tmpVal.constructor !== String) {
            return null;
        }

        // Links
        if (this._rgx.wt_link.test(tmpVal)) {
            // handle [[link|name]]
            {
                const matches = tmpVal.match(this._rgx.wt_named_link) || [];

                for (const match of matches) {
                    if (!tmpVal) {
                        continue;
                    }

                    const repl_op = tmpVal.replace(
                        match,
                        match
                            .slice(2, -2) // get rid of [[ and ]]
                            .split('|') // split by delimiter
                            .pop(), // get right side xx|yy -> yy
                    );

                    if (!repl_op) {
                        continue;
                    }

                    tmpVal = repl_op;
                }
            }

            // handle [[link]]
            {
                const matches = tmpVal.match(this._rgx.wt_link) || [];

                for (const match of matches) {
                    if (!tmpVal) {
                        continue;
                    }

                    const repl_op = tmpVal.replace(
                        match,
                        match.slice(2, -2), // get rid of [[ and ]]
                    );

                    if (!repl_op) {
                        continue;
                    }

                    tmpVal = repl_op;
                }
            }
        }

        // <sub>, <sup>
        if (tmpVal) {
            tmpVal =
                tmpVal.replace(
                    this._rgx.wt_sub_sup,
                    '$1',
                );
        }

        return tmpVal;
    }

    _sanitizedIfNeeded(propName, propValue) {
        if (!propValue) {
            return propValue;
        }

        if (this._sanitizers.has(propName)) {
            return this._sanitizers.get(propName)(propValue);
        }

        return propValue;
    }

    parse(propSet) {
        const procPropMap = {};

        propSet[1].map(([_propName, { prop }]) => {
            const propName = _propName.toLowerCase();

            let rx;

            switch (true) {
                /* durations */
                case this._rgx.range_dur.test(propName):
                    rx = this._rgx.range_dur.exec(propName);

                    _.set(
                        procPropMap,
                        `roa.${rx[1]}.duration.${rx[3]}.${rx[2]}`,
                        prop,
                    );

                    break;

                /* doses */

                case this._rgx.range_dose.test(propName):
                    rx = this._rgx.range_dose.exec(propName);

                    _.set(
                        procPropMap,
                        `roa.${rx[1]}.dose.${rx[3]}.${rx[2]}`,
                        prop,
                    );

                    break;

                case this._rgx.def_dose.test(propName):
                    rx = this._rgx.def_dose.exec(propName);

                    _.set(
                        procPropMap,
                        `roa.${rx[1]}.dose.${rx[2]}`,
                        prop,
                    );

                    break;

                case this._rgx.def_bioavailability.test(propName):
                    rx = this._rgx.def_bioavailability.exec(propName);

                    _.set(
                        procPropMap,
                        `roa.${rx[1]}.bioavailability.${rx[2]}`,
                        prop,
                    );

                    break;

                /* units */

                case this._rgx.dose_units.test(propName):
                    rx = this._rgx.dose_units.exec(propName);

                    _.set(
                        procPropMap,
                        `roa.${rx[1]}.dose.units`,
                        prop,
                    );

                    break;

                case this._rgx.roa_time_units.test(propName):
                    rx = this._rgx.roa_time_units.exec(propName);

                    _.set(
                        procPropMap,
                        `roa.${rx[1]}.duration.${rx[2]}.units`,
                        prop,
                    );

                    break;

                /* meta */

                case this._rgx.meta_tolerance_time.test(propName):
                    rx = this._rgx.meta_tolerance_time.exec(propName);

                    _.set(
                        procPropMap,
                        `tolerance.${rx[1]}`,
                        prop,
                    );

                    break;
            }

            if (this._flatMetaProps.has(propName)) {
                const mappedPropName = this._flatMetaProps.get(propName);

                _.set(
                    procPropMap,
                    mappedPropName,
                    this._sanitizedIfNeeded(mappedPropName, prop),
                );
            }

            if (this._mappedMetaProps.has(propName)) {
                try {
                    rx = this._mappedMetaProps.get(propName)(prop);
                } catch {
                    rx = [propName, null]
                }

                _.set(
                    procPropMap,
                    rx[0],
                    this._sanitizedIfNeeded(rx[0], rx[1]),
                );
            }
        });

        // new ROA interface
        const rawROAMap = _.get(procPropMap, 'roa', {});

        const mappedROAs =
            _.chain(rawROAMap)
                .keys()
                .map(key =>
                    _.merge(
                        _.get(rawROAMap, key),
                        { name: key },
                    ),
                )
                .value();

        _.assign(
            procPropMap,
            {
                roas: mappedROAs,
            },
        );

        return procPropMap;
    }

    parseFromSMW(obj) {
        return this.parse(
            this._smwDataArbitrator
                .parse(obj),
        );
    }
}

module.exports = PWPropParser;
