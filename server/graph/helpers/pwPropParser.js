'use strict';

const _ = require('lodash');

class PWPropParser {
    constructor({smwDataArbitrator}) {
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
            wt_prop_glob: /\[\[(.*?)\]\]/g,
            wt_prop: /\[\[(.*?)\]\]/
        };

        this._flatMetaProps = new Map([
            ['addiction_potential', 'addictionPotential'],
            ['dangerousinteraction', 'dangerousInteractions'],
            ['effect', 'effects']
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
                        this._rgx.wt_prop.exec(item)[1]
                    );

                return [propTarget, mappedCrossTolerance];
            }],
            ['featured', prop => ['featured', prop === 't']],
            ['psychoactive_class', prop => (['class.psychoactive', [].concat(prop)])],
            ['chemical_class', prop => (['class.chemical', [].concat(prop)])]
        ]);
    }

    parse(propSet) {
        const procPropMap = {};

        propSet[1].map(([_propName, {prop}]) => {
            const propName = _propName.toLowerCase();

            let rx;

            switch (true) {
                /* durations */
                case this._rgx.range_dur.test(propName):
                    rx = this._rgx.range_dur.exec(propName);

                    _.set(procPropMap, `roa.${rx[1]}.duration.${rx[3]}.${rx[2]}`, prop);

                    break;

                /* doses */

                case this._rgx.range_dose.test(propName):
                    rx = this._rgx.range_dose.exec(propName);

                    _.set(procPropMap, `roa.${rx[1]}.dose.${rx[3]}.${rx[2]}`, prop);

                    break;

                case this._rgx.def_dose.test(propName):
                    rx = this._rgx.def_dose.exec(propName);

                    _.set(procPropMap, `roa.${rx[1]}.dose.${rx[2]}`, prop);

                    break;

                case this._rgx.def_bioavailability.test(propName):
                    rx = this._rgx.def_bioavailability.exec(propName);

                    _.set(procPropMap, `roa.${rx[1]}.bioavailability.${rx[2]}`, prop);

                    break;

                /* units */

                case this._rgx.dose_units.test(propName):
                    rx = this._rgx.dose_units.exec(propName);

                    _.set(procPropMap, `roa.${rx[1]}.dose.units`, prop);

                    break;

                case this._rgx.roa_time_units.test(propName):
                    rx = this._rgx.roa_time_units.exec(propName);

                    _.set(procPropMap, `roa.${rx[1]}.duration.${rx[2]}.units`, prop);

                    break;

                /* meta */

                case this._rgx.meta_tolerance_time.test(propName):
                    rx = this._rgx.meta_tolerance_time.exec(propName);

                    _.set(procPropMap, `tolerance.${rx[1]}`, prop);

                    break;
            }

            if (this._flatMetaProps.has(propName)) {
                _.set(procPropMap, this._flatMetaProps.get(propName), prop);
            }

            if (this._mappedMetaProps.has(propName)) {
                rx = this._mappedMetaProps.get(propName)(prop);

                _.set(procPropMap, rx[0], rx[1]);
            }
        });

        // new ROA interface
        const rawROAMap = _.get(procPropMap, 'roa', {});

        const mappedROAs = _.chain(rawROAMap)
                            .keys()
                            .map(key => _.merge(_.get(rawROAMap, key), {name: key}))
                            .value();

        _.assign(procPropMap, { roas: mappedROAs });

        return procPropMap;
    }

    parseFromSMW(obj) {
        return this.parse(this._smwDataArbitrator.parse(obj));
    }
}

module.exports = PWPropParser;
