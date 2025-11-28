'use strict';

const express = require('express');
const { ApolloServer } = require('apollo-server-express');
const { ApolloServerPluginLandingPageGraphQLPlayground } = require('apollo-server-core');

const StaleWhileRevalidateCache = require('../core/cache');
const PsychonautWikiConnector = require('../services/psychonautwiki/connector');
const SMWTransformer = require('../services/psychonautwiki/transformer');
const WikitextParser = require('../services/psychonautwiki/parser');
const PsychonautWikiService = require('../services/psychonautwiki/service');
const PlebisciteDatabase = require('../services/plebiscite/database');

const createSchema = require('./schema');
const createResolvers = require('./resolvers');

async function startServer({ config, logger }) {
    const app = express();

    // Initialize Core Services
    const cache = new StaleWhileRevalidateCache({ 
        logger, 
        ttl: config.psychonautWiki.cacheTtl 
    });

    // Initialize PW Services
    const pwConnector = new PsychonautWikiConnector({ config, logger, cache });
    const smwTransformer = new SMWTransformer();
    const pwParser = new WikitextParser({ smwTransformer });
    const pwService = new PsychonautWikiService({ 
        connector: pwConnector, 
        parser: pwParser, 
        config, 
        logger 
    });

    // Initialize Plebiscite (if enabled)
    let plebisciteDB = null;
    if (config.features.plebiscite.enabled) {
        plebisciteDB = new PlebisciteDatabase({ config, logger });
    }

    // GraphQL Setup
    const typeDefs = createSchema({ config });
    const resolvers = createResolvers({ config });

    const server = new ApolloServer({
        typeDefs,
        resolvers,
        context: ({ req }) => ({
            pwService,
            plebisciteDB,
            logger,
            req
        }),
        plugins: [
            ApolloServerPluginLandingPageGraphQLPlayground({
                endpoint: '/'
            })
        ],
        introspection: true,
        debug: config.env === 'development'
    });

    await server.start();
    server.applyMiddleware({ app, path: '/' });

    return { app, server };
}

module.exports = { startServer };
