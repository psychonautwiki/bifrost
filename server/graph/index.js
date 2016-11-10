'use strict';

const ItemEdge = require('./itemType');

module.exports = ({log}) => {
    const itemEdge = new ItemEdge({
        name: 'query',
        description: 'The Lots base edge'
    }, {});

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
