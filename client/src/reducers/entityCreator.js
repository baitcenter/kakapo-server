
import { ACTIONS } from '../actions'

const initialState = {
  creatingEntities: false,
  entitiesDirty: false,

  error: null,
  tableName: null,

  primaryKey: 0,
  columns: { 0: null },
}

const entityCreator = (state = initialState, action) => {
  switch (action.type) {
    case ACTIONS.ENTITY_CREATOR.ERROR:
      return {
        ...state,
        error: action.msg,
      }
    case ACTIONS.ENTITY_CREATOR.CLEAR_ERROR:
      return {
        ...state,
        error: null,
      }
    case ACTIONS.ENTITY_CREATOR.CLEAR_DIRTY_ENTITIES:
      return {
        ...state,
        entitiesDirty: false,
      }
    case ACTIONS.ENTITY_CREATOR.START_CREATING_ENTITIES:
      return {
        ...state,
        creatingEntities: true,
      }
    case ACTIONS.ENTITY_CREATOR.COMMIT_TABLE_CHANGES:
      return {
        ...state,
        creatingEntities: false,
        entitiesDirty: true,
      }
    case ACTIONS.ENTITY_CREATOR.SET_TABLE_NAME:
      return {
        ...state,
        tableName: action.name,
      }
    case ACTIONS.ENTITY_CREATOR.MODIFY_STATE:
      return {
        ...state,
        columns: action.columns,
        primaryKey: action.primaryKey,
      }
    default:
      return state
  }
}

export default entityCreator