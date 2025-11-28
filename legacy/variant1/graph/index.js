'use strict';

const ItemEdge = require('./resolvers');

module.exports = ({log}) => {
    const itemEdge = new ItemEdge();

    return {
        schema: itemEdge.schema,
        root({userData}) {
            return {
                // used by storage to provide
                // request-level caching
                //db: EnrichedStorage.withCache(new Map(), log),
                log, userData
            };
        }
    };
};
