'use strict';

require('dotenv').config();

const config = require('./config');
const logger = require('./core/logger');
const asciiArt = require('./utils/ascii');
const { startServer } = require('./graphql/server');

/**
 * Application Bootstrap
 */
(async () => {
    try {
        // Print startup banner
        logger.info(asciiArt);

        // Initialize and start the server
        const { app, server } = await startServer({ config, logger });

        const host = config.server.host;
        const port = config.server.port;

        app.listen(port, host, () => {
            logger.info({ type: 'server' }, `Bifrost Online: http://${host}:${port}${server.graphqlPath}`);
        });

    } catch (err) {
        logger.fatal({ err }, 'Failed to start Bifrost server');
        process.exit(1);
    }
})();

// Global error handlers
process.on('unhandledRejection', (reason, promise) => {
    logger.error({ err: reason }, 'Unhandled Rejection at:', promise);
});

process.on('uncaughtException', (err) => {
    logger.fatal({ err }, 'Uncaught Exception');
    process.exit(1);
});
