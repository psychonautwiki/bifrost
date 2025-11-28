'use strict';

const _ = require('lodash');

/**
 * Feature Flag Management
 */
const features = {
    plebiscite: {
        enabled: _.has(process.env, 'PLEBISCITE'),
        requiredEnv: ['MONGO_URL'],
        optionalEnv: {
            MONGO_COLLECTION: 'plebiscite',
            MONGO_DB: 'bifrost' // Default DB if not specified in URL or separate var
        }
    }
};

/**
 * Validate active features
 */
Object.entries(features).forEach(([featureName, featureConfig]) => {
    if (featureConfig.enabled) {
        featureConfig.requiredEnv.forEach(envVar => {
            if (!process.env[envVar]) {
                throw new Error(`Feature '${featureName}' is enabled but missing required env var: '${envVar}'`);
            }
        });
    }
});

module.exports = {
    env: process.env.NODE_ENV || 'development',
    server: {
        host: process.env.HOST || '0.0.0.0',
        port: process.env.PORT || 3000,
    },
    logging: {
        level: process.env.LOG_LEVEL || 'info',
        name: 'bifrost'
    },
    psychonautWiki: {
        apiUrl: 'https://psychonautwiki.org/w/api.php',
        cdnUrl: 'https://psychonautwiki.org/',
        thumbSize: 100,
        cacheTtl: 24 * 60 * 60 * 1000, // 24 hours
    },
    features: {
        plebiscite: {
            enabled: features.plebiscite.enabled,
            mongo: features.plebiscite.enabled ? {
                url: process.env.MONGO_URL,
                db: process.env.MONGO_DB || features.plebiscite.optionalEnv.MONGO_DB,
                collection: process.env.MONGO_COLLECTION || features.plebiscite.optionalEnv.MONGO_COLLECTION
            } : null
        }
    }
};
