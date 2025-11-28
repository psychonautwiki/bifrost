'use strict';

const { MongoClient } = require('mongodb');

class PlebisciteDatabase {
    constructor({ config, logger }) {
        this._config = config.features.plebiscite.mongo;
        this._logger = logger.child({ component: 'PlebisciteDB' });
        this._client = null;
        this._db = null;
    }

    async connect() {
        if (this._db) return this._db;

        try {
            this._logger.info('Connecting to MongoDB...');
            this._client = await MongoClient.connect(this._config.url, {
                useNewUrlParser: true,
                useUnifiedTopology: true
            });
            
            this._db = this._client.db(this._config.db);
            this._logger.info('Connected to MongoDB');
            return this._db;
        } catch (err) {
            this._logger.error({ err }, 'Failed to connect to MongoDB');
            throw err;
        }
    }

    async find({ substance, offset, limit }) {
        const db = await this.connect();
        const collection = db.collection(this._config.collection);

        const query = {};
        if (substance) {
            query['substanceInfo.substance'] = substance;
        }

        return collection.find(query)
            .sort({ 'meta.published': -1 })
            .skip(offset || 0)
            .limit(limit || 50)
            .toArray();
    }
}

module.exports = PlebisciteDatabase;
