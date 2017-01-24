'use strict';

const _ = require('lodash');

const features = new Map([
    ['plebiscite', new Map([
        ['required', ['MONGO_URL']],
        ['optional', [['MONGO_COLLECTION', 'plebiscite']]]
    ])]
]);

class Features {
    constructor(featureSet) {
        this._activeFeatures = new Set();

        this._initialize(featureSet);
    }

    _envIsSet(flag) {
        return _.has(process.env, flag.toUpperCase());
    }

    envGet(flag) {
        return _.get(process.env, flag.toUpperCase());
    }

    _initialize(featureSet) {
        featureSet.forEach((props, feature) => {
            if (!this._envIsSet(feature)) {
                return;
            }

            const required = props.get('required');

            required.some(prop => {
                if (!this._envIsSet(prop)) {
                    throw new Error(`Feature '${feature}' is active, but the flag '${prop}' is missing. `);
                }
            });

            const optional = props.get('optional');

            optional.forEach(([prop, def]) => {
                if (!this._envIsSet(prop)) {
                    process.env[prop] = def;
                }
            });

            this._activeFeatures.add(feature);
        });
    }

    has(feature) {
        return this._activeFeatures.has(feature);
    }
}

module.exports = new Features(features);