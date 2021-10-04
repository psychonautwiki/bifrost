'use strict';

const {MongoClient} = require('mongodb');

const MONGO_URL = process.env.MONGO_URL;
const MONGO_COLLECTION = process.env.MONGO_COLLECTION;

const mdb_delayed = MongoClient.connect(MONGO_URL);

class Plebiscite {
    constructor({db}) {
        this._db = db;
    }

    async _getCollection() {
        return (await this._db).collection(MONGO_COLLECTION);
    }

    async find({substance, offset, limit}) {
        const collection = await this._getCollection();

        const query = {};

        if (!substance === true) {
            query['substanceInfo.substance'] = substance;
        }

        return collection.find(query)
            .sort({'meta.published': -1})
            .skip(offset)
            .limit(limit)
            .toArray();
    }
}

module.exports = new Plebiscite({db: mdb_delayed});
